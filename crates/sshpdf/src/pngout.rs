//! Drawing a cell grid as a PNG, the way a terminal would show it: block
//! glyphs from their masks, box drawing as lines, text with a bundled
//! monospace font (DejaVu Sans Mono). For checking visuals without a
//! terminal.

use std::io::Write;
use std::path::Path;

use ab_glyph::{Font, FontRef, PxScale, ScaleFont, point};
use anyhow::{Context, Result};
use sshpdf_layout::boxchars::{DOUBLE, HEAVY, NONE, arms_of};
use sshpdf_raster::{SH, SW};
use sshpdf_term::palette::xterm_rgb;
use sshpdf_term::{Attrs, Cell, CellGrid, Color, Wide};

static FONT: &[u8] = include_bytes!("../assets/DejaVuSansMono.ttf");

/// Colors standing in for the terminal's defaults.
#[derive(Clone, Copy, Debug)]
pub struct PngTheme {
    pub fg: [u8; 3],
    pub bg: [u8; 3],
}

impl PngTheme {
    pub fn new(dark: bool) -> PngTheme {
        if dark {
            PngTheme {
                fg: [220, 220, 215],
                bg: [28, 28, 32],
            }
        } else {
            PngTheme {
                fg: [24, 24, 24],
                bg: [250, 250, 248],
            }
        }
    }
}

struct Canvas {
    w: usize,
    h: usize,
    px: Vec<[u8; 3]>,
}

impl Canvas {
    fn blend(&mut self, x: i64, y: i64, color: [u8; 3], a: f32) {
        if x < 0 || y < 0 || x as usize >= self.w || y as usize >= self.h || a <= 0.0 {
            return;
        }
        let p = &mut self.px[y as usize * self.w + x as usize];
        let a = a.min(1.0);
        for i in 0..3 {
            p[i] = (p[i] as f32 * (1.0 - a) + color[i] as f32 * a + 0.5) as u8;
        }
    }

    fn rect(&mut self, x0: i64, y0: i64, x1: i64, y1: i64, color: [u8; 3]) {
        for y in y0.max(0)..y1.min(self.h as i64) {
            for x in x0.max(0)..x1.min(self.w as i64) {
                self.px[y as usize * self.w + x as usize] = color;
            }
        }
    }
}

fn resolve(c: Color, default: [u8; 3]) -> [u8; 3] {
    match c {
        Color::Default => default,
        Color::Rgb(r, g, b) => [r, g, b],
        Color::Indexed(i) => xterm_rgb(i),
    }
}

/// Draw a box-drawing character from its arms.
fn draw_box(cv: &mut Canvas, x: i64, y: i64, cw: i64, ch: i64, arms: [u8; 4], fg: [u8; 3]) {
    let (cx, cy) = (x + cw / 2, y + ch / 2);
    let light = (cw / 10).max(1);
    let heavy = (cw / 4).max(light + 1);
    let gap = (cw / 5).max(2);
    // (start, end) offsets along the arm, thickness, lane offsets
    let lanes = |w: u8| -> (i64, Vec<i64>) {
        match w {
            HEAVY => (heavy, vec![0]),
            DOUBLE => (light, vec![-gap / 2 - light / 2, gap / 2 + light / 2]),
            _ => (light, vec![0]),
        }
    };
    let [up, down, left, right] = arms;
    // Vertical arms.
    for (w, from, to) in [(up, y, cy + 1), (down, cy, y + ch)] {
        if w == NONE {
            continue;
        }
        let (t, offs) = lanes(w);
        for o in offs {
            let x0 = cx + o - t / 2;
            cv.rect(x0, from, x0 + t, to, fg);
        }
    }
    for (w, from, to) in [(left, x, cx + 1), (right, cx, x + cw)] {
        if w == NONE {
            continue;
        }
        let (t, offs) = lanes(w);
        for o in offs {
            let y0 = cy + o - t / 2;
            cv.rect(from, y0, to, y0 + t, fg);
        }
    }
}

/// Where a glyph goes: the cell's top-left pixel, the cell width and how
/// many cells it spans.
#[derive(Clone, Copy)]
struct Spot {
    x: i64,
    y: i64,
    cw: i64,
    span: i64,
}

struct Painter<'f> {
    font: FontRef<'f>,
    scale: PxScale,
    ascent: f32,
}

impl Painter<'_> {
    /// Draw `c` in the cell(s) at `spot`; false if the font lacks it.
    fn glyph(&self, cv: &mut Canvas, c: char, spot: Spot, fg: [u8; 3], attrs: Attrs) -> bool {
        let Spot { x, y, cw, span } = spot;
        let id = self.font.glyph_id(c);
        if id.0 == 0 {
            return false;
        }
        let glyph = id.with_scale_and_position(self.scale, point(x as f32, y as f32 + self.ascent));
        let Some(outlined) = self.font.outline_glyph(glyph) else {
            return true; // a blank glyph
        };
        let b = outlined.px_bounds();
        let italic = attrs.contains(Attrs::ITALIC);
        let bold = attrs.contains(Attrs::BOLD);
        // Centre narrow glyphs in their cells.
        let adv = self.font.as_scaled(self.scale).h_advance(id);
        let dx = ((cw * span) as f32 - adv) / 2.0;
        let baseline = y as f32 + self.ascent;
        outlined.draw(|gx, gy, cov| {
            let py = b.min.y + gy as f32;
            let shear = if italic { (baseline - py) * 0.2 } else { 0.0 };
            let px = (b.min.x + gx as f32 + dx + shear).round() as i64;
            cv.blend(px, py as i64, fg, cov);
            if bold {
                cv.blend(px + 1, py as i64, fg, cov);
            }
        });
        true
    }
}

/// Render the grid to RGB pixels: `cols * cell_w` x `rows * cell_h`.
pub fn render(grid: &CellGrid, cell_w: u32, cell_h: u32, theme: PngTheme) -> (usize, usize, Vec<u8>) {
    let (cw, ch) = (cell_w as i64, cell_h as i64);
    let mut cv = Canvas {
        w: grid.cols() as usize * cell_w as usize,
        h: grid.rows() as usize * cell_h as usize,
        px: vec![theme.bg; grid.cols() as usize * cell_w as usize * grid.rows() as usize * cell_h as usize],
    };
    let font = FontRef::try_from_slice(FONT).expect("bundled font is valid");
    let scale = PxScale::from(cell_h as f32 * 0.98);
    let ascent = font.as_scaled(scale).ascent();
    let painter = Painter { font, scale, ascent };
    for r in 0..grid.rows() {
        for c in 0..grid.cols() {
            let cell: &Cell = grid.get(c, r).expect("in bounds");
            if cell.wide == Wide::Trail {
                continue;
            }
            let span = if cell.wide == Wide::Lead { 2 } else { 1 };
            let (x, y) = (c as i64 * cw, r as i64 * ch);
            let mut fg = resolve(cell.fg, theme.fg);
            let mut bg = resolve(cell.bg, theme.bg);
            if cell.attrs.contains(Attrs::REVERSE) {
                std::mem::swap(&mut fg, &mut bg);
            }
            if cell.attrs.contains(Attrs::DIM) {
                fg = [0, 1, 2].map(|i| ((fg[i] as u16 + bg[i] as u16) / 2) as u8);
            }
            cv.rect(x, y, x + cw * span, y + ch, bg);
            let g = cell.ch;
            if g != ' ' {
                if let Some(mask) = sshpdf_raster::glyphs::mask_for(g) {
                    for py in 0..ch {
                        for px in 0..cw {
                            let sx = (px * SW as i64 / cw) as usize;
                            let sy = (py * SH as i64 / ch) as usize;
                            cv.blend(x + px, y + py, fg, mask[sy * SW + sx]);
                        }
                    }
                } else if let Some(arms) = arms_of(g) {
                    draw_box(&mut cv, x, y, cw, ch, arms, fg);
                } else if g == '┄' || g == '┆' {
                    let t = (cw / 10).max(1);
                    for k in 0..3 {
                        if g == '┄' {
                            let x0 = x + k * cw / 3;
                            cv.rect(x0, y + ch / 2, x0 + cw / 4, y + ch / 2 + t, fg);
                        } else {
                            let y0 = y + k * ch / 3;
                            cv.rect(x + cw / 2, y0, x + cw / 2 + t, y0 + ch / 4, fg);
                        }
                    }
                } else if !painter.glyph(&mut cv, g, Spot { x, y, cw, span }, fg, cell.attrs) {
                    // Not in the font: an empty box.
                    let (x1, y1) = (x + cw * span - 2, y + ch - 3);
                    cv.rect(x + 1, y + 3, x1, y + 4, fg);
                    cv.rect(x + 1, y1 - 1, x1, y1, fg);
                    cv.rect(x + 1, y + 3, x + 2, y1, fg);
                    cv.rect(x1 - 1, y + 3, x1, y1, fg);
                }
            }
            if cell.mark != '\0' {
                painter.glyph(&mut cv, cell.mark, Spot { x, y, cw, span }, fg, cell.attrs);
            }
            if cell.attrs.contains(Attrs::UNDERLINE) {
                let ul = match cell.ul {
                    Color::Default => fg,
                    c => resolve(c, fg),
                };
                cv.rect(x, y + ch - 2, x + cw * span, y + ch - 1, ul);
            }
        }
    }
    let mut out = Vec::with_capacity(cv.px.len() * 3);
    for p in &cv.px {
        out.extend_from_slice(p);
    }
    (cv.w, cv.h, out)
}

/// Write RGB pixels as a PNG file.
pub fn write_png(path: &Path, w: usize, h: usize, rgb: &[u8]) -> Result<()> {
    let file = std::fs::File::create(path).with_context(|| format!("creating {}", path.display()))?;
    let mut buf = std::io::BufWriter::new(file);
    let mut enc = png::Encoder::new(&mut buf, w as u32, h as u32);
    enc.set_color(png::ColorType::Rgb);
    enc.set_depth(png::BitDepth::Eight);
    enc.set_compression(png::Compression::Fast);
    let mut writer = enc.write_header()?;
    writer.write_image_data(rgb)?;
    writer.finish()?;
    buf.flush()?;
    Ok(())
}
