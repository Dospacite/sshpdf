//! Combining the text, line and raster layers into one cell grid.
//!
//! Text wins over lines, lines win over raster. Where text sits on an image
//! or a filled area, the cell keeps the raster background and the text gets
//! a contrasting color.

use std::collections::HashSet;

use sshpdf_doc::{Doc, Exclusions, PageGraphics, PageText, PointPt, RectPt, SizePt};
use sshpdf_raster::color::rgb_to_lab;
use sshpdf_raster::{GlyphSet, Ink, LabImage, Options as RasterOptions, SH, SW, Theme, optimize_lab};
use sshpdf_term::{Attrs, Cell, CellGrid, Color, Tier};

use crate::lines::{LineCell, LineLayer, line_layer};
use crate::mapping::{CellRect, Mapping};
use crate::textlayer::{TextCell, TextLayer, text_layer};

/// How a page is drawn.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum Mode {
    /// Text as characters, lines as box drawing, the rest raster.
    Layout,
    /// Everything through the raster layer.
    Raster,
}

/// Options for turning a page into cells.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct RenderOptions {
    pub tier: Tier,
    pub shapes: bool,
    pub theme: Theme,
    /// Error diffusion between cells (with a limited palette).
    pub diffusion: bool,
}

/// One page at one zoom: the parts that don't depend on the viewport.
#[derive(Clone, Debug)]
pub struct PageLayout {
    pub page: usize,
    pub size: SizePt,
    pub mapping: Mapping,
    pub mode: Mode,
    pub text: TextLayer,
    pub lines: LineLayer,
    pub exclusions: Exclusions,
    /// Image areas: shown in their real colors.
    pub photos: Vec<RectPt>,
}

impl PageLayout {
    /// Layout mode: place text and lines, and exclude them from the raster.
    pub fn layout(page: usize, size: SizePt, text: &PageText, graphics: &PageGraphics, mapping: Mapping) -> PageLayout {
        let text_layer = text_layer(text, &mapping);
        let occupied: HashSet<(i32, i32)> = text_layer
            .cells
            .iter()
            .flat_map(|(&(c, r), t)| [(c, r), (if t.wide { c + 1 } else { c }, r)])
            .collect();
        let lines = line_layer(graphics, &mapping, &occupied);
        let exclusions = Exclusions {
            text: true,
            keep_text: text_layer.dropped.iter().map(|(r, _)| *r).collect(),
            paths: lines.excluded.clone(),
        };
        debug_assert!(exclusions.keep_text.iter().all(|r| r.height() > 0.0));
        PageLayout {
            page,
            size,
            mapping,
            mode: Mode::Layout,
            text: text_layer,
            lines,
            exclusions,
            photos: graphics.images.clone(),
        }
    }

    /// Raster mode: the whole page through the raster layer.
    pub fn raster(page: usize, size: SizePt, graphics: &PageGraphics, mapping: Mapping) -> PageLayout {
        PageLayout {
            page,
            size,
            mapping,
            mode: Mode::Raster,
            text: TextLayer::default(),
            lines: LineLayer::default(),
            exclusions: Exclusions::none(),
            photos: graphics.images.clone(),
        }
    }

    /// The page's size in cells.
    pub fn grid_size(&self) -> (u32, u32) {
        self.mapping.page_cells(self.size)
    }

    /// Whether all text in rows `r0..r1` could be placed as characters.
    pub fn legible_in(&self, r0: i32, r1: i32) -> bool {
        self.mode == Mode::Raster || self.text.legible_in(r0, r1)
    }

    fn is_photo(&self, p: PointPt) -> bool {
        self.photos.iter().any(|r| r.contains(p))
    }
}

/// The cell for text drawn over a background of ink `under`.
pub fn text_cell(tc: &TextCell, under: Ink, theme: &Theme, underline: bool) -> Cell {
    let mut attrs = Attrs::NONE;
    if tc.flags & sshpdf_doc::char_flags::BOLD != 0 {
        attrs |= Attrs::BOLD;
    }
    if tc.flags & sshpdf_doc::char_flags::ITALIC != 0 {
        attrs |= Attrs::ITALIC;
    }
    if underline {
        attrs |= Attrs::UNDERLINE;
    }
    let lab = rgb_to_lab(tc.color);
    let (fg, bg) = match under {
        Ink::Paper => (theme.text_color(lab, None), Color::Default),
        // On black ink: reverse video shows the default colors swapped.
        Ink::Black => {
            attrs |= Attrs::REVERSE;
            (Color::Default, Color::Default)
        }
        Ink::Color(c) => (theme.text_color(lab, Some(c)), theme.literal(c)),
    };
    Cell {
        ch: tc.ch,
        mark: tc.mark,
        fg,
        bg,
        ul: Color::Default,
        attrs,
        wide: sshpdf_term::Wide::No,
    }
}

fn line_cell(lc: &LineCell, under: Ink, theme: &Theme) -> Cell {
    let lab = rgb_to_lab(lc.color);
    match under {
        Ink::Paper => Cell::new(lc.ch, theme.text_color(lab, None), Color::Default),
        Ink::Black => Cell::new(lc.ch, Color::Default, Color::Default).with_attrs(Attrs::REVERSE),
        Ink::Color(c) => Cell::new(lc.ch, theme.text_color(lab, Some(c)), theme.literal(c)),
    }
}

/// Render the cells of `window` (page-grid coordinates) of a page.
pub fn render_cells(doc: &dyn Doc, layout: &PageLayout, window: CellRect, opts: &RenderOptions) -> CellGrid {
    let (cols, rows) = (window.cols as usize, window.rows as usize);
    let mut grid = CellGrid::new(window.cols, window.rows);
    if cols == 0 || rows == 0 {
        return grid;
    }
    let theme = &opts.theme;
    // What each cell's background shows, for drawing text and lines on it.
    // Cells off the page stay blank (the terminal background), also with
    // `paper`, so the page edge shows.
    let mut under = vec![Ink::Paper; cols * rows];
    let m = &layout.mapping;
    let region = m.cell_rect_to_page(&window);
    let page_rect = RectPt::new(0.0, 0.0, layout.size.w, layout.size.h);
    if region.intersects(&page_rect) {
        let (pw, ph) = (cols * SW, rows * SH);
        let img = doc.render(layout.page, region, pw as u32, ph as u32, &layout.exclusions);
        let set = GlyphSet::get(opts.tier, opts.shapes);
        let extras = theme.quant_extras();
        let ropts = RasterOptions {
            depth: theme.depth,
            diffusion: opts.diffusion,
            blur: true,
            extras: [extras.first().copied(), extras.get(1).copied()],
            coherence: 0.02,
        };
        if img.width as usize == pw && img.height as usize == ph {
            // Page colors → the colors the terminal shows (photos stay real).
            let mut lab = LabImage::from_rgba(&img.data, pw, ph);
            if !theme.paper {
                let sx = pw as f32 / region.width();
                let sy = ph as f32 / region.height();
                let photos: Vec<(f32, f32, f32, f32)> = layout
                    .photos
                    .iter()
                    .map(|r| {
                        (
                            (r.x0 - region.x0) * sx,
                            (r.y0 - region.y0) * sy,
                            (r.x1 - region.x0) * sx,
                            (r.y1 - region.y0) * sy,
                        )
                    })
                    .collect();
                lab.map(|x, y, c| {
                    let (fx, fy) = (x as f32 + 0.5, y as f32 + 0.5);
                    if photos
                        .iter()
                        .any(|&(x0, y0, x1, y1)| fx >= x0 && fx < x1 && fy >= y0 && fy < y1)
                    {
                        c
                    } else {
                        theme.to_display(c)
                    }
                });
            }
            let cells = optimize_lab(&lab, cols, rows, set, &ropts);
            for (i, rc) in cells.iter().enumerate() {
                let (c, r) = (i % cols, i / cols);
                let (x, y) = m.to_page(window.col as f32 + c as f32 + 0.5, window.row as f32 + r as f32 + 0.5);
                let center = PointPt { x, y };
                if !page_rect.contains(center) {
                    continue;
                }
                let photo = layout.is_photo(center);
                grid.set(c as u16, r as u16, theme.raster_cell(rc, set, photo));
                let main = if set.coverage(rc.glyph as usize) > 0.5 {
                    rc.fg
                } else {
                    rc.bg
                };
                under[i] = theme.classify_display(main, photo);
            }
        }
    }

    if layout.mode == Mode::Raster {
        return grid;
    }
    let inside = |col: i32, row: i32| window.contains(col, row);
    for (&(col, row), lc) in &layout.lines.cells {
        if inside(col, row) {
            let (c, r) = ((col - window.col) as usize, (row - window.row) as usize);
            grid.set(c as u16, r as u16, line_cell(lc, under[r * cols + c], theme));
        }
    }
    // Rules that ran under text: underline the row, text or not.
    for (&(col, row), &color) in &layout.lines.underlines {
        if !inside(col, row) || layout.text.cells.contains_key(&(col, row)) {
            continue;
        }
        let (c, r) = ((col - window.col) as u16, (row - window.row) as u16);
        if let Some(cell) = grid.get_mut(c, r) {
            if cell.ch == ' ' {
                cell.fg = theme.text_color(rgb_to_lab(color), None);
            }
            cell.attrs |= Attrs::UNDERLINE;
        }
    }
    for (&(col, row), tc) in &layout.text.cells {
        if !inside(col, row) {
            continue;
        }
        let (c, r) = ((col - window.col) as usize, (row - window.row) as usize);
        let underline = layout.lines.underlines.contains_key(&(col, row));
        let cell = text_cell(tc, under[r * cols + c], theme, underline);
        grid.set(c as u16, r as u16, cell);
        if tc.wide && c + 1 < cols {
            // The right half keeps the same background.
            if let Some(trail) = grid.get_mut(c as u16 + 1, r as u16) {
                trail.bg = cell.bg;
            }
        }
    }
    grid
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn text_on_paper_and_on_color() {
        let theme = Theme::default();
        let tc = TextCell {
            ch: 'a',
            mark: '\0',
            flags: sshpdf_doc::char_flags::BOLD,
            color: [0, 0, 0],
            wide: false,
            src: 0,
        };
        let c = text_cell(&tc, Ink::Paper, &theme, false);
        assert_eq!((c.fg, c.bg), (Color::Default, Color::Default));
        assert!(c.attrs.contains(Attrs::BOLD));
        let c = text_cell(&tc, Ink::Color(rgb_to_lab([0, 0, 120])), &theme, true);
        assert!(matches!(c.bg, Color::Rgb(..)));
        assert!(c.attrs.contains(Attrs::UNDERLINE));
        let c = text_cell(&tc, Ink::Black, &theme, false);
        assert!(c.attrs.contains(Attrs::REVERSE));
    }
}
