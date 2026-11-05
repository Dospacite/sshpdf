//! From page colors to terminal colors.
//!
//! By default the page blends into the terminal: page white becomes the
//! terminal's default background and black ink its default foreground, so
//! the page follows the user's theme, light or dark. On a dark terminal,
//! lightness is turned around (ink light, paper dark) while colors keep
//! their hue. Photos keep their real colors. With `paper`, everything keeps
//! its real colors, white paper included.
//!
//! The raster layer works in **display space**: the rendered page is first
//! mapped to the colors the terminal should show ([`Theme::to_display`]),
//! then optimized and quantized; finally colors equal to the assumed
//! terminal defaults become the real defaults ([`Theme::classify_display`]).

use sshpdf_term::{Attrs, Cell, Color, ColorDepth};

use crate::color::{Lab, Palette, chroma, dist2, lab_to_rgb, rgb_to_lab};
use crate::glyphs::GlyphSet;
use crate::optimize::RasterCell;

/// How colors are mapped.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Theme {
    /// Draw real white paper instead of the terminal background.
    pub paper: bool,
    /// Whether the terminal background is dark (assumed when unknown).
    pub dark: bool,
    pub depth: ColorDepth,
}

impl Default for Theme {
    fn default() -> Self {
        Theme {
            paper: false,
            dark: true,
            depth: ColorDepth::True,
        }
    }
}

/// How a color shows up in the terminal.
#[derive(Clone, Copy, Debug, PartialEq)]
pub enum Ink {
    /// The terminal's default background (page white).
    Paper,
    /// The terminal's default foreground (black ink).
    Black,
    /// A real color (OKLab, display space).
    Color(Lab),
}

/// Page white as seen in OKLab.
const WHITE: Lab = [1.0, 0.0, 0.0];
const BLACK: Lab = [0.0, 0.0, 0.0];

/// Assumed lightness of a dark theme's background and foreground (the real
/// ones are unknown: no terminal queries).
const DARK_BG_L: f32 = 0.2;
const DARK_FG_L: f32 = 0.88;

/// How close (OKLab distance) a display color must be to an assumed
/// terminal default to become that default.
const BG_EPS: f32 = 0.025;
const FG_EPS: f32 = 0.04;

impl Theme {
    /// The color the terminal background is assumed to have (display space).
    pub fn default_bg(&self) -> Lab {
        if self.paper || !self.dark {
            WHITE
        } else {
            [DARK_BG_L, 0.0, 0.0]
        }
    }

    /// The color the terminal foreground is assumed to have (display space).
    pub fn default_fg(&self) -> Lab {
        if self.paper || !self.dark {
            BLACK
        } else {
            [DARK_FG_L, 0.0, 0.0]
        }
    }

    /// Map a page color (made for white paper) to the color the terminal
    /// should show. On a dark terminal, lightness runs from the foreground
    /// (black ink) down to the background (paper): grays keep their place
    /// between ink and paper; colors keep their hue and get at least medium
    /// lightness. Identity on a light terminal and with `paper`.
    pub fn to_display(&self, lab: Lab) -> Lab {
        if !self.dark || self.paper {
            return lab;
        }
        let [l, a, b] = lab;
        let t = (chroma(lab) / 0.12).min(1.0);
        let between = DARK_FG_L + (DARK_BG_L - DARK_FG_L) * l;
        let lifted = l.max(0.66);
        [between * (1.0 - t) + lifted * t, a, b]
    }

    /// Extra quantization targets: the terminal's default colors, so flat
    /// paper and ink become exactly the defaults in every color depth.
    pub fn quant_extras(&self) -> Vec<Lab> {
        if self.paper {
            Vec::new()
        } else {
            vec![self.default_bg(), self.default_fg()]
        }
    }

    /// What a display-space color becomes: the terminal default when it
    /// matches the assumed one, otherwise a real color. Photos never use the
    /// foreground default.
    pub fn classify_display(&self, lab: Lab, photo: bool) -> Ink {
        if self.paper {
            return Ink::Color(lab);
        }
        if dist2(lab, self.default_bg()) < BG_EPS * BG_EPS {
            Ink::Paper
        } else if !photo && dist2(lab, self.default_fg()) < FG_EPS * FG_EPS {
            Ink::Black
        } else {
            Ink::Color(lab)
        }
    }

    /// What a page color becomes, for text and lines (generous thresholds:
    /// dark gray text is still "black" text).
    pub fn classify(&self, lab: Lab, photo: bool) -> Ink {
        if self.paper || photo {
            return Ink::Color(lab);
        }
        let ch = chroma(lab);
        if lab[0] > 0.93 && ch < 0.035 {
            Ink::Paper
        } else if lab[0] < 0.32 && ch < 0.06 {
            Ink::Black
        } else {
            Ink::Color(self.to_display(lab))
        }
    }

    /// The terminal color for a real color, quantized to the color depth.
    pub fn literal(&self, lab: Lab) -> Color {
        match Palette::for_depth(self.depth) {
            None => {
                let [r, g, b] = lab_to_rgb(lab);
                Color::Rgb(r, g, b)
            }
            Some(p) => Color::Indexed(p.index[p.nearest(lab)]),
        }
    }

    /// Terminal color for text of page color `text`, drawn on `bg`
    /// (`None`: the terminal's own background; otherwise the color the cell
    /// shows, in display space).
    pub fn text_color(&self, text: Lab, bg: Option<Lab>) -> Color {
        match bg {
            None => match self.classify(text, false) {
                Ink::Black => Color::Default,
                // White text on white paper is invisible in the PDF too;
                // it usually sits on something drawn by the raster layer.
                Ink::Paper => {
                    if self.paper {
                        self.literal(text)
                    } else {
                        Color::Default
                    }
                }
                Ink::Color(c) => self.literal(c),
            },
            Some(bg) => {
                let c = if self.paper {
                    text
                } else {
                    match self.classify(text, false) {
                        Ink::Black => self.default_fg(),
                        Ink::Paper => self.default_bg(),
                        Ink::Color(c) => c,
                    }
                };
                self.literal(contrast(c, bg))
            }
        }
    }

    /// The color an ink shows (display space), for drawing text over it.
    pub fn shown(&self, ink: Ink) -> Lab {
        match ink {
            Ink::Paper => self.default_bg(),
            Ink::Black => self.default_fg(),
            Ink::Color(c) => c,
        }
    }

    /// Turn a raster cell (display space) into a terminal cell.
    pub fn raster_cell(&self, rc: &RasterCell, set: &GlyphSet, photo: bool) -> Cell {
        let g = rc.glyph as usize;
        let cov = set.coverage(g);
        let fg = self.classify_display(rc.fg, photo);
        let bg = self.classify_display(rc.bg, photo);
        // A cell showing (almost) one color.
        if cov < 1e-3 || same(fg, bg, self) {
            return self.solid(if cov < 1e-3 { bg } else { fg });
        }
        if cov > 1.0 - 1e-3 {
            return self.solid(fg);
        }
        let ch = set.glyphs[g].ch;
        // The terminal's default colors only work in their own slots: paper
        // as background, black ink as foreground. When the glyph's ink is
        // paper (or the background is black ink), draw the complement glyph
        // with the colors in the other slots; without a complement, reverse
        // video swaps how the slots display.
        let swapped = |fg_slot: Color, bg_slot: Color| match set.complement(g) {
            Some(c) => Cell::new(set.glyphs[c].ch, fg_slot, bg_slot),
            None => Cell::new(ch, fg_slot, bg_slot).with_attrs(Attrs::REVERSE),
        };
        match (fg, bg) {
            (Ink::Black, Ink::Paper) => Cell::new(ch, Color::Default, Color::Default),
            (Ink::Paper, Ink::Black) => swapped(Color::Default, Color::Default),
            (Ink::Color(f), Ink::Paper) => Cell::new(ch, self.literal(f), Color::Default),
            (Ink::Paper, Ink::Color(b)) => swapped(self.literal(b), Color::Default),
            (Ink::Black, Ink::Color(b)) => Cell::new(ch, Color::Default, self.literal(b)),
            (Ink::Color(f), Ink::Black) => swapped(Color::Default, self.literal(f)),
            (Ink::Color(f), Ink::Color(b)) => Cell::new(ch, self.literal(f), self.literal(b)),
            // Both the same kind: handled by `same` above.
            (Ink::Paper, Ink::Paper) | (Ink::Black, Ink::Black) => self.solid(fg),
        }
    }

    /// A cell filled with one color.
    pub fn solid(&self, ink: Ink) -> Cell {
        match ink {
            Ink::Paper => Cell::BLANK,
            Ink::Black => Cell::new(' ', Color::Default, Color::Default).with_attrs(Attrs::REVERSE),
            Ink::Color(c) => Cell::new(' ', Color::Default, self.literal(c)),
        }
    }

    /// The ink of a page color given as sRGB (paper space).
    pub fn ink_of_rgb(&self, rgb: [u8; 3], photo: bool) -> Ink {
        self.classify(rgb_to_lab(rgb), photo)
    }
}

fn same(a: Ink, b: Ink, theme: &Theme) -> bool {
    match (a, b) {
        (Ink::Paper, Ink::Paper) | (Ink::Black, Ink::Black) => true,
        (Ink::Color(x), Ink::Color(y)) => theme.literal(x) == theme.literal(y),
        _ => false,
    }
}

/// Keep `c`'s hue but make sure its lightness differs enough from `bg`.
pub fn contrast(c: Lab, bg: Lab) -> Lab {
    const MIN: f32 = 0.38;
    if (c[0] - bg[0]).abs() >= MIN {
        return c;
    }
    let l = if bg[0] > 0.55 {
        (bg[0] - MIN).max(0.0)
    } else {
        (bg[0] + MIN).min(1.0)
    };
    [l, c[1], c[2]]
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::glyphs::GlyphSet;
    use sshpdf_term::Tier;

    #[test]
    fn white_and_black_become_terminal_defaults() {
        let t = Theme::default();
        assert_eq!(t.classify(rgb_to_lab([255, 255, 255]), false), Ink::Paper);
        assert_eq!(t.classify(rgb_to_lab([250, 250, 248]), false), Ink::Paper);
        assert_eq!(t.classify(rgb_to_lab([0, 0, 0]), false), Ink::Black);
        assert_eq!(t.classify(rgb_to_lab([30, 30, 35]), false), Ink::Black);
        assert!(matches!(t.classify(rgb_to_lab([255, 255, 255]), true), Ink::Color(_)));
        let paper = Theme { paper: true, ..t };
        assert!(matches!(
            paper.classify(rgb_to_lab([255, 255, 255]), false),
            Ink::Color(_)
        ));
    }

    #[test]
    fn dark_theme_keeps_colors_visible() {
        let t = Theme::default();
        let blue = rgb_to_lab([0, 0, 238]);
        let Ink::Color(c) = t.classify(blue, false) else {
            panic!()
        };
        assert!(c[0] >= 0.6, "{c:?}");
        assert!((c[1] - blue[1]).abs() < 1e-6 && (c[2] - blue[2]).abs() < 1e-6);
        // A light gray fill becomes a gray just above the background.
        let g = t.to_display(rgb_to_lab([220, 220, 220]));
        assert!(g[0] > DARK_BG_L && g[0] < 0.35, "{g:?}");
        // Darker grays stay between background and foreground, in order.
        let d = t.to_display(rgb_to_lab([90, 90, 90]));
        assert!(d[0] > g[0] && d[0] < DARK_FG_L, "{d:?}");
        // Paper and ink land exactly on the assumed defaults.
        assert_eq!(
            t.classify_display(t.to_display(rgb_to_lab([255, 255, 255])), false),
            Ink::Paper
        );
        assert_eq!(
            t.classify_display(t.to_display(rgb_to_lab([0, 0, 0])), false),
            Ink::Black
        );
        assert!(matches!(t.classify_display(g, false), Ink::Color(_)));
    }

    #[test]
    fn text_color_contrasts_with_cell_background() {
        let t = Theme::default();
        assert_eq!(t.text_color(rgb_to_lab([0, 0, 0]), None), Color::Default);
        // Black text on a dark blue box becomes light.
        let Color::Rgb(r, g, b) = t.text_color(rgb_to_lab([0, 0, 0]), Some(rgb_to_lab([0, 0, 90]))) else {
            panic!()
        };
        assert!(rgb_to_lab([r, g, b])[0] > 0.5);
    }

    #[test]
    fn ink_on_paper_uses_default_colors() {
        let t = Theme::default();
        let set = GlyphSet::get(Tier::Half, false);
        let upper = set.glyphs.iter().position(|g| g.ch == '▀').unwrap();
        let (ink, paper) = (t.default_fg(), t.default_bg());
        let rc = RasterCell {
            glyph: upper as u16,
            fg: ink,
            bg: paper,
        };
        assert_eq!(
            t.raster_cell(&rc, set, false),
            Cell::new('▀', Color::Default, Color::Default)
        );
        // Paper on top, ink below: the complement glyph.
        let rc = RasterCell {
            glyph: upper as u16,
            fg: paper,
            bg: ink,
        };
        assert_eq!(
            t.raster_cell(&rc, set, false),
            Cell::new('▄', Color::Default, Color::Default)
        );
    }

    #[test]
    fn no_complement_falls_back_to_reverse_video() {
        let t = Theme::default();
        let set = GlyphSet::get(Tier::Braille, false);
        let rc = RasterCell {
            glyph: 1,
            fg: t.default_bg(),
            bg: t.default_fg(),
        };
        let c = t.raster_cell(&rc, set, false);
        assert!(c.attrs.contains(Attrs::REVERSE));
    }
}
