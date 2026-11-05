//! Page space ↔ cell grid.
//!
//! Zoom is defined by the **row pitch**: how many points one terminal row
//! covers. The column pitch is the row pitch times the cell aspect (cell
//! width / height, about 0.5), divided by an optional horizontal stretch.
//! All layers use this one mapping.
//!
//! The stretch exists for text zoom: proportional body text averages about
//! 0.4 em per character, while a terminal cell is about 0.6 em wide at one
//! row per line, so a line of text needs ~1.3-1.7x more columns than the
//! page width gives it. Text zoom widens the grid by that factor so every
//! line fits on its row (images get wider too; raster mode and the lens use
//! no stretch).

use sshpdf_doc::{PageText, RectPt, SizePt};

/// Points per row at 100% zoom: a 10 pt font with 12 pt leading gets one
/// row per line.
pub const POINTS_PER_ROW_AT_100: f32 = 12.0;

/// Largest horizontal stretch for text zoom.
pub const MAX_STRETCH: f32 = 2.2;

/// Smallest and largest row pitch (points per row).
pub const MIN_ROW_PITCH: f32 = 0.25;
pub const MAX_ROW_PITCH: f32 = 400.0;

/// Maps page points to cells. Cell (0, 0) has its top-left corner at the
/// page's left edge and `y_offset` points from its top (0 or slightly above).
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Mapping {
    /// Points per terminal row.
    pub row_pitch: f32,
    /// Cell width divided by cell height.
    pub cell_aspect: f32,
    /// Horizontal stretch (1 = true aspect).
    pub x_stretch: f32,
    /// Page y of the top of row 0, in `(-row_pitch, 0]`. Text zoom picks it
    /// so body text lines sit in the middle of their rows instead of on a
    /// row boundary, where a small baseline shift would move a line into its
    /// neighbour's row.
    pub y_offset: f32,
}

/// A rectangle of cells: `col`, `row` may be negative (outside the page).
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub struct CellRect {
    pub col: i32,
    pub row: i32,
    pub cols: u16,
    pub rows: u16,
}

impl CellRect {
    pub fn new(col: i32, row: i32, cols: u16, rows: u16) -> CellRect {
        CellRect { col, row, cols, rows }
    }

    pub fn contains(&self, col: i32, row: i32) -> bool {
        col >= self.col && row >= self.row && col < self.col + self.cols as i32 && row < self.row + self.rows as i32
    }
}

impl Mapping {
    pub fn new(row_pitch: f32, cell_aspect: f32) -> Mapping {
        Mapping {
            row_pitch: row_pitch.clamp(MIN_ROW_PITCH, MAX_ROW_PITCH),
            cell_aspect: cell_aspect.clamp(0.2, 2.0),
            x_stretch: 1.0,
            y_offset: 0.0,
        }
    }

    /// The same mapping with a horizontal stretch.
    pub fn with_stretch(self, x_stretch: f32) -> Mapping {
        Mapping {
            x_stretch: x_stretch.clamp(1.0, MAX_STRETCH),
            ..self
        }
    }

    /// Points per column.
    pub fn col_pitch(&self) -> f32 {
        self.row_pitch * self.cell_aspect / self.x_stretch
    }

    /// Zoom in percent: 100% is 12 points per row.
    pub fn zoom_percent(&self) -> f32 {
        POINTS_PER_ROW_AT_100 * 100.0 / self.row_pitch
    }

    pub fn from_zoom_percent(percent: f32, cell_aspect: f32) -> Mapping {
        Mapping::new(POINTS_PER_ROW_AT_100 * 100.0 / percent.max(0.1), cell_aspect)
    }

    /// The page width fills `cols` columns.
    pub fn fit_width(page: SizePt, cols: u16, cell_aspect: f32) -> Mapping {
        let col_pitch = page.w / cols.max(1) as f32;
        Mapping::new(col_pitch / cell_aspect, cell_aspect)
    }

    /// The whole page fits in `cols` x `rows`.
    pub fn fit_page(page: SizePt, cols: u16, rows: u16, cell_aspect: f32) -> Mapping {
        let by_height = page.h / rows.max(1) as f32;
        let by_width = page.w / cols.max(1) as f32 / cell_aspect;
        Mapping::new(by_height.max(by_width), cell_aspect)
    }

    /// Text zoom: one row per line of body text, from the page's most
    /// common baseline gap, stretched so body lines fit their width.
    pub fn text_zoom(text: &PageText, cell_aspect: f32) -> Option<Mapping> {
        let spacing = body_line_spacing(text)?;
        let m = Mapping::new(spacing, cell_aspect);
        let m = m.with_stretch(text_stretch(text, &m));
        Some(Mapping {
            y_offset: text_phase(text, &m),
            ..m
        })
    }

    /// Zoom by a factor (> 1 zooms in), keeping the stretch.
    pub fn zoomed(&self, factor: f32) -> Mapping {
        Mapping::new(self.row_pitch / factor, self.cell_aspect).with_stretch(self.x_stretch)
    }

    /// Page point → fractional cell coordinates (col, row).
    pub fn to_cell(&self, x: f32, y: f32) -> (f32, f32) {
        (self.col_of(x), self.row_of(y))
    }

    /// Fractional column of a page x.
    pub fn col_of(&self, x: f32) -> f32 {
        x / self.col_pitch()
    }

    /// Fractional row of a page y.
    pub fn row_of(&self, y: f32) -> f32 {
        (y - self.y_offset) / self.row_pitch
    }

    /// Fractional cell coordinates → page point.
    pub fn to_page(&self, col: f32, row: f32) -> (f32, f32) {
        (col * self.col_pitch(), row * self.row_pitch + self.y_offset)
    }

    /// The page area covered by a rectangle of cells.
    pub fn cell_rect_to_page(&self, r: &CellRect) -> RectPt {
        let (x0, y0) = self.to_page(r.col as f32, r.row as f32);
        let (x1, y1) = self.to_page((r.col + r.cols as i32) as f32, (r.row + r.rows as i32) as f32);
        RectPt::new(x0, y0, x1, y1)
    }

    /// Size of a page in cells (rounded up).
    pub fn page_cells(&self, page: SizePt) -> (u32, u32) {
        let (c, r) = self.to_cell(page.w, page.h);
        ((c - 1e-3).ceil().max(1.0) as u32, (r - 1e-3).ceil().max(1.0) as u32)
    }
}

/// The vertical offset that puts body text lines in the middle of rows:
/// the circular mean of where their x-height centres fall within a row.
pub fn text_phase(text: &PageText, m: &Mapping) -> f32 {
    let body = crate::reflow::body_size(text);
    let (mut sx, mut sy) = (0.0f32, 0.0f32);
    for l in text.lines() {
        if (l.size - body).abs() > 0.6 {
            continue;
        }
        let y = l.baseline - 0.33 * l.size;
        let phase = (y / m.row_pitch).rem_euclid(1.0);
        let a = phase * std::f32::consts::TAU;
        let w = l.char_count() as f32;
        sx += w * a.cos();
        sy += w * a.sin();
    }
    if sx == 0.0 && sy == 0.0 {
        return 0.0;
    }
    let mean = sy.atan2(sx).rem_euclid(std::f32::consts::TAU) / std::f32::consts::TAU;
    let mut off = (mean - 0.5) * m.row_pitch;
    if off > 0.0 {
        off -= m.row_pitch;
    }
    off
}

/// The most common gap between consecutive baselines of body text,
/// ignoring outliers (paragraph gaps, headings, superscript lines).
pub fn body_line_spacing(text: &PageText) -> Option<f32> {
    // The dominant font size, weighted by characters.
    let mut sizes: Vec<(f32, usize)> = Vec::new();
    for l in text.lines() {
        let s = (l.size * 2.0).round() / 2.0;
        match sizes.iter_mut().find(|(v, _)| *v == s) {
            Some((_, n)) => *n += l.char_count(),
            None => sizes.push((s, l.char_count())),
        }
    }
    let body = sizes.iter().max_by_key(|(_, n)| *n)?.0;
    // Gaps between consecutive lines of the same block with body-sized text.
    let mut hist: Vec<(i32, usize)> = Vec::new();
    for b in &text.blocks {
        for w in b.lines.windows(2) {
            let (a, c) = (&w[0], &w[1]);
            if (a.size - body).abs() > 0.6 || (c.size - body).abs() > 0.6 {
                continue;
            }
            let gap = c.baseline - a.baseline;
            if gap < 0.8 * body || gap > 2.5 * body {
                continue;
            }
            let bin = (gap * 4.0).round() as i32;
            match hist.iter_mut().find(|(v, _)| *v == bin) {
                Some((_, n)) => *n += 1,
                None => hist.push((bin, 1)),
            }
        }
    }
    match hist.iter().max_by_key(|(bin, n)| (*n, -*bin)) {
        Some((bin, _)) => Some(*bin as f32 / 4.0),
        None => Some(body * 1.2),
    }
}

/// How much wider the grid must be for body text to fit its lines at `m`:
/// the ratio of the columns a line needs (characters plus one space between
/// words) to the columns its page width gives it, for most body lines.
pub fn text_stretch(text: &PageText, m: &Mapping) -> f32 {
    let cp = m.row_pitch * m.cell_aspect;
    let body = crate::reflow::body_size(text);
    let mut ratios: Vec<f32> = Vec::new();
    for l in text.lines() {
        if (l.size - body).abs() > 0.6 || l.words.len() < 3 {
            continue;
        }
        let needed: usize = l
            .words
            .iter()
            .map(|w| {
                crate::textlayer::word_cells(w)
                    .iter()
                    .map(|c| if c.wide { 2 } else { 1 })
                    .sum::<usize>()
            })
            .sum::<usize>()
            + l.words.len()
            - 1;
        let avail = l.bbox.width() / cp;
        if avail > 4.0 {
            ratios.push(needed as f32 / avail);
        }
    }
    if ratios.is_empty() {
        return 1.0;
    }
    ratios.sort_by(f32::total_cmp);
    // Nearly every line should fit without overflowing: an overflowing line
    // in a two-column page runs into the other column.
    let p = ratios[(ratios.len() * 19 / 20).min(ratios.len() - 1)];
    (p * 1.03).clamp(1.0, MAX_STRETCH)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn pitches_and_zoom() {
        let m = Mapping::new(12.0, 0.5);
        assert_eq!(m.col_pitch(), 6.0);
        assert!((m.zoom_percent() - 100.0).abs() < 1e-4);
        assert_eq!(m.to_cell(60.0, 24.0), (10.0, 2.0));
        assert_eq!(m.to_page(10.0, 2.0), (60.0, 24.0));
        let z = m.zoomed(2.0);
        assert_eq!(z.row_pitch, 6.0);
        let s = m.with_stretch(1.5);
        assert!((s.col_pitch() - 4.0).abs() < 1e-5);
        assert_eq!(s.zoomed(2.0).x_stretch, 1.5);
        assert_eq!(m.with_stretch(9.0).x_stretch, MAX_STRETCH);
        assert!((Mapping::from_zoom_percent(200.0, 0.5).row_pitch - 6.0).abs() < 1e-4);
    }

    #[test]
    fn fits() {
        let letter = SizePt { w: 612.0, h: 792.0 };
        let w = Mapping::fit_width(letter, 102, 0.5);
        assert!((w.col_pitch() - 6.0).abs() < 1e-4);
        assert_eq!(w.page_cells(letter), (102, 66));
        let p = Mapping::fit_page(letter, 80, 24, 0.5);
        let (c, r) = p.page_cells(letter);
        assert!(c <= 80 && r <= 24, "{c}x{r}");
    }

    #[test]
    fn cell_rects() {
        let m = Mapping::new(10.0, 0.5);
        let r = m.cell_rect_to_page(&CellRect::new(2, 3, 4, 5));
        assert_eq!(r, RectPt::new(10.0, 30.0, 30.0, 80.0));
        let o = Mapping { y_offset: -4.0, ..m };
        assert_eq!(o.to_cell(0.0, 6.0), (0.0, 1.0));
        assert_eq!(o.to_page(0.0, 1.0), (0.0, 6.0));
        assert_eq!(o.page_cells(SizePt { w: 10.0, h: 17.0 }), (2, 3));
        assert!(CellRect::new(-1, -1, 2, 2).contains(0, 0));
        assert!(!CellRect::new(-1, -1, 2, 2).contains(1, 0));
    }
}
