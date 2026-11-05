//! The cell grid: what the rendering layers produce and the writers send.

use std::ops::{BitOr, BitOrAssign};

use unicode_width::UnicodeWidthChar;

/// A terminal color as it is sent: the terminal's own default, an RGB triple
/// or a palette index.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Hash)]
pub enum Color {
    /// The terminal's default foreground or background (SGR 39 / 49).
    #[default]
    Default,
    Rgb(u8, u8, u8),
    Indexed(u8),
}

/// Text attributes (SGR 1, 2, 3, 4, 7).
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Hash)]
pub struct Attrs(u8);

impl Attrs {
    pub const NONE: Attrs = Attrs(0);
    pub const BOLD: Attrs = Attrs(1);
    pub const ITALIC: Attrs = Attrs(2);
    pub const UNDERLINE: Attrs = Attrs(4);
    pub const REVERSE: Attrs = Attrs(8);
    pub const DIM: Attrs = Attrs(16);

    pub fn contains(self, other: Attrs) -> bool {
        self.0 & other.0 == other.0
    }

    pub fn is_empty(self) -> bool {
        self.0 == 0
    }

    pub fn remove(&mut self, other: Attrs) {
        self.0 &= !other.0;
    }

    pub fn bits(self) -> u8 {
        self.0
    }
}

impl BitOr for Attrs {
    type Output = Attrs;
    fn bitor(self, rhs: Attrs) -> Attrs {
        Attrs(self.0 | rhs.0)
    }
}

impl BitOrAssign for Attrs {
    fn bitor_assign(&mut self, rhs: Attrs) {
        self.0 |= rhs.0;
    }
}

/// How a cell takes part in a double-width character.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Hash)]
pub enum Wide {
    /// A normal single-width cell.
    #[default]
    No,
    /// The left half of a double-width character; holds the character.
    Lead,
    /// The right half of a double-width character; holds nothing.
    Trail,
}

/// One terminal cell.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub struct Cell {
    pub ch: char,
    /// A combining mark drawn on `ch`, or `'\0'` for none.
    pub mark: char,
    pub fg: Color,
    pub bg: Color,
    /// Underline color, used together with [`Attrs::UNDERLINE`].
    pub ul: Color,
    pub attrs: Attrs,
    pub wide: Wide,
}

impl Default for Cell {
    fn default() -> Self {
        Cell::BLANK
    }
}

impl Cell {
    pub const BLANK: Cell = Cell {
        ch: ' ',
        mark: '\0',
        fg: Color::Default,
        bg: Color::Default,
        ul: Color::Default,
        attrs: Attrs::NONE,
        wide: Wide::No,
    };

    pub fn new(ch: char, fg: Color, bg: Color) -> Cell {
        Cell {
            ch,
            fg,
            bg,
            ..Cell::BLANK
        }
    }

    pub fn with_attrs(mut self, attrs: Attrs) -> Cell {
        self.attrs = attrs;
        self
    }

    /// The text this cell sends: the character plus its combining mark.
    pub fn symbol(&self, buf: &mut [u8; 8]) -> usize {
        let mut n = self.ch.encode_utf8(buf).len();
        if self.mark != '\0' {
            n += self.mark.encode_utf8(&mut buf[n..]).len();
        }
        n
    }
}

/// Display width of a character in cells (0, 1 or 2).
pub fn char_width(c: char) -> usize {
    c.width().unwrap_or(0)
}

/// A rectangular grid of cells.
#[derive(Clone, Debug, PartialEq)]
pub struct CellGrid {
    cols: u16,
    rows: u16,
    cells: Vec<Cell>,
}

impl CellGrid {
    pub fn new(cols: u16, rows: u16) -> CellGrid {
        CellGrid {
            cols,
            rows,
            cells: vec![Cell::BLANK; cols as usize * rows as usize],
        }
    }

    pub fn cols(&self) -> u16 {
        self.cols
    }

    pub fn rows(&self) -> u16 {
        self.rows
    }

    fn idx(&self, col: u16, row: u16) -> usize {
        row as usize * self.cols as usize + col as usize
    }

    pub fn get(&self, col: u16, row: u16) -> Option<&Cell> {
        (col < self.cols && row < self.rows).then(|| &self.cells[self.idx(col, row)])
    }

    pub fn get_mut(&mut self, col: u16, row: u16) -> Option<&mut Cell> {
        if col < self.cols && row < self.rows {
            let i = self.idx(col, row);
            Some(&mut self.cells[i])
        } else {
            None
        }
    }

    pub fn row(&self, row: u16) -> &[Cell] {
        let start = row as usize * self.cols as usize;
        &self.cells[start..start + self.cols as usize]
    }

    pub fn cells(&self) -> &[Cell] {
        &self.cells
    }

    pub fn cells_mut(&mut self) -> &mut [Cell] {
        &mut self.cells
    }

    pub fn fill(&mut self, cell: Cell) {
        self.cells.fill(cell);
    }

    /// Clear the other half of a wide character that overlaps `(col, row)`.
    fn break_wide(&mut self, col: u16, row: u16) {
        let i = self.idx(col, row);
        match self.cells[i].wide {
            Wide::Lead if col + 1 < self.cols => {
                let j = i + 1;
                let bg = self.cells[j].bg;
                self.cells[j] = Cell::new(' ', Color::Default, bg);
            }
            Wide::Trail if col > 0 => {
                let j = i - 1;
                let bg = self.cells[j].bg;
                self.cells[j] = Cell::new(' ', Color::Default, bg);
            }
            _ => {}
        }
    }

    /// Put a cell at `(col, row)`. A double-width character also takes the
    /// cell to its right; if there is no room, a space goes there instead.
    /// Returns the number of columns used (0 if out of bounds).
    pub fn set(&mut self, col: u16, row: u16, mut cell: Cell) -> u16 {
        if col >= self.cols || row >= self.rows {
            return 0;
        }
        self.break_wide(col, row);
        if char_width(cell.ch) == 2 {
            if col + 1 >= self.cols {
                cell.ch = ' ';
                cell.mark = '\0';
                cell.wide = Wide::No;
                let i = self.idx(col, row);
                self.cells[i] = cell;
                return 1;
            }
            self.break_wide(col + 1, row);
            cell.wide = Wide::Lead;
            let i = self.idx(col, row);
            self.cells[i] = cell;
            self.cells[i + 1] = Cell {
                ch: ' ',
                mark: '\0',
                wide: Wide::Trail,
                ..cell
            };
            2
        } else {
            cell.wide = Wide::No;
            let i = self.idx(col, row);
            self.cells[i] = cell;
            1
        }
    }

    /// Write a string starting at `(col, row)`, clipped at the right edge.
    /// Zero-width characters attach to the previous cell as its mark.
    /// Returns the number of columns used.
    pub fn put_str(&mut self, col: u16, row: u16, s: &str, template: Cell) -> u16 {
        let mut x = col;
        let mut last: Option<u16> = None;
        for c in s.chars() {
            let w = char_width(c);
            if w == 0 {
                if let Some(lx) = last
                    && let Some(cell) = self.get_mut(lx, row)
                    && cell.mark == '\0'
                {
                    cell.mark = c;
                }
                continue;
            }
            if x >= self.cols {
                break;
            }
            let used = self.set(
                x,
                row,
                Cell {
                    ch: c,
                    mark: '\0',
                    ..template
                },
            );
            last = Some(x);
            x += used;
        }
        x - col
    }

    /// Copy `src` into this grid with its top-left corner at `(col, row)`.
    pub fn blit(&mut self, src: &CellGrid, col: i32, row: i32) {
        for sr in 0..src.rows {
            let r = row + sr as i32;
            if r < 0 || r >= self.rows as i32 {
                continue;
            }
            for sc in 0..src.cols {
                let c = col + sc as i32;
                if c < 0 || c >= self.cols as i32 {
                    continue;
                }
                let cell = *src.get(sc, sr).expect("in bounds");
                let i = self.idx(c as u16, r as u16);
                self.cells[i] = cell;
            }
        }
        self.fix_edges();
    }

    /// Cut out a `cols` x `rows` window whose top-left is at `(col, row)`
    /// (may be negative or reach past the grid; the outside is blank).
    pub fn crop(&self, col: i32, row: i32, cols: u16, rows: u16) -> CellGrid {
        let mut out = CellGrid::new(cols, rows);
        out.blit(self, -col, -row);
        out
    }

    /// Replace half-cut wide characters (at the grid edges after a crop or
    /// blit) with spaces.
    fn fix_edges(&mut self) {
        for r in 0..self.rows {
            for c in 0..self.cols {
                let i = self.idx(c, r);
                let cell = self.cells[i];
                let broken = match cell.wide {
                    Wide::Lead => c + 1 >= self.cols || self.cells[i + 1].wide != Wide::Trail,
                    Wide::Trail => c == 0 || self.cells[i - 1].wide != Wide::Lead,
                    Wide::No => false,
                };
                if broken {
                    self.cells[i] = Cell::new(' ', Color::Default, cell.bg);
                }
            }
        }
    }

    /// The plain text of one row, without styles (trailing spaces kept).
    pub fn row_text(&self, row: u16) -> String {
        let mut s = String::new();
        for cell in self.row(row) {
            if cell.wide == Wide::Trail {
                continue;
            }
            s.push(cell.ch);
            if cell.mark != '\0' {
                s.push(cell.mark);
            }
        }
        s
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn wide_chars_take_two_cells() {
        let mut g = CellGrid::new(5, 1);
        let used = g.put_str(0, 0, "a漢b", Cell::BLANK);
        assert_eq!(used, 4);
        assert_eq!(g.get(1, 0).unwrap().wide, Wide::Lead);
        assert_eq!(g.get(2, 0).unwrap().wide, Wide::Trail);
        assert_eq!(g.row_text(0), "a漢b ");
    }

    #[test]
    fn overwriting_half_a_wide_char_clears_the_other_half() {
        let mut g = CellGrid::new(4, 1);
        g.put_str(0, 0, "漢", Cell::BLANK);
        g.set(1, 0, Cell::new('x', Color::Default, Color::Default));
        assert_eq!(g.get(0, 0).unwrap().ch, ' ');
        assert_eq!(g.get(0, 0).unwrap().wide, Wide::No);
        assert_eq!(g.row_text(0), " x  ");
    }

    #[test]
    fn wide_char_at_right_edge_becomes_space() {
        let mut g = CellGrid::new(3, 1);
        g.put_str(2, 0, "漢", Cell::BLANK);
        assert_eq!(g.row_text(0), "   ");
    }

    #[test]
    fn crop_cuts_wide_chars_cleanly() {
        let mut g = CellGrid::new(4, 1);
        g.put_str(0, 0, "漢字", Cell::BLANK);
        let c = g.crop(1, 0, 2, 1);
        assert_eq!(c.row_text(0), "  ");
        let c = g.crop(-1, 0, 4, 1);
        assert_eq!(c.row_text(0), " 漢 ");
    }

    #[test]
    fn combining_marks_attach_to_previous_cell() {
        let mut g = CellGrid::new(3, 1);
        let used = g.put_str(0, 0, "e\u{301}x", Cell::BLANK);
        assert_eq!(used, 2);
        assert_eq!(g.get(0, 0).unwrap().mark, '\u{301}');
        assert_eq!(g.row_text(0), "e\u{301}x ");
    }
}
