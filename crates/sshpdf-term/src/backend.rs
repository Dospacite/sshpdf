//! A ratatui backend with sshpdf's output rules: grouped SGR changes, a
//! byte count per frame, and the robust output mode.

use std::io::{self, Write};

use ratatui::backend::{Backend, ClearType, WindowSize};
use ratatui::buffer::Cell as RCell;
use ratatui::crossterm::{cursor, queue, terminal};
use ratatui::layout::{Position, Size};
use ratatui::style::{Color as RColor, Modifier};
use unicode_width::UnicodeWidthStr;

use crate::caps::ColorDepth;
use crate::cell::{Attrs, Cell, Color, Wide};
use crate::sgr::{Pen, transition};

/// Characters every terminal (and tmux) agrees are exactly one cell wide:
/// printable ASCII, Box Drawing and Block Elements.
pub fn is_width_safe(c: char) -> bool {
    matches!(c, ' '..='~' | '\u{2500}'..='\u{259F}')
}

/// Convert a ratatui color to a sshpdf color.
pub fn from_ratatui_color(c: RColor) -> Color {
    match c {
        RColor::Reset => Color::Default,
        RColor::Rgb(r, g, b) => Color::Rgb(r, g, b),
        RColor::Indexed(i) => Color::Indexed(i),
        RColor::Black => Color::Indexed(0),
        RColor::Red => Color::Indexed(1),
        RColor::Green => Color::Indexed(2),
        RColor::Yellow => Color::Indexed(3),
        RColor::Blue => Color::Indexed(4),
        RColor::Magenta => Color::Indexed(5),
        RColor::Cyan => Color::Indexed(6),
        RColor::Gray => Color::Indexed(7),
        RColor::DarkGray => Color::Indexed(8),
        RColor::LightRed => Color::Indexed(9),
        RColor::LightGreen => Color::Indexed(10),
        RColor::LightYellow => Color::Indexed(11),
        RColor::LightBlue => Color::Indexed(12),
        RColor::LightMagenta => Color::Indexed(13),
        RColor::LightCyan => Color::Indexed(14),
        RColor::White => Color::Indexed(15),
    }
}

/// Convert a sshpdf color to a ratatui color.
pub fn to_ratatui_color(c: Color) -> RColor {
    match c {
        Color::Default => RColor::Reset,
        Color::Rgb(r, g, b) => RColor::Rgb(r, g, b),
        Color::Indexed(i) => RColor::Indexed(i),
    }
}

/// Copy a sshpdf cell into a ratatui buffer cell.
pub fn to_ratatui_cell(cell: &Cell, out: &mut RCell) {
    if cell.wide == Wide::Trail {
        // ratatui skips the cell after a wide character by itself.
        out.reset();
        out.bg = to_ratatui_color(cell.bg);
        return;
    }
    let mut buf = [0u8; 8];
    let n = cell.symbol(&mut buf);
    out.set_symbol(std::str::from_utf8(&buf[..n]).unwrap_or(" "));
    out.fg = to_ratatui_color(cell.fg);
    out.bg = to_ratatui_color(cell.bg);
    out.underline_color = to_ratatui_color(cell.ul);
    let mut m = Modifier::empty();
    for (a, f) in [
        (Attrs::BOLD, Modifier::BOLD),
        (Attrs::ITALIC, Modifier::ITALIC),
        (Attrs::UNDERLINE, Modifier::UNDERLINED),
        (Attrs::REVERSE, Modifier::REVERSED),
        (Attrs::DIM, Modifier::DIM),
    ] {
        if cell.attrs.contains(a) {
            m |= f;
        }
    }
    out.modifier = m;
}

fn pen_of(cell: &RCell) -> Pen {
    let mut attrs = Attrs::NONE;
    for (f, a) in [
        (Modifier::BOLD, Attrs::BOLD),
        (Modifier::ITALIC, Attrs::ITALIC),
        (Modifier::UNDERLINED, Attrs::UNDERLINE),
        (Modifier::REVERSED, Attrs::REVERSE),
        (Modifier::DIM, Attrs::DIM),
    ] {
        if cell.modifier.contains(f) {
            attrs |= a;
        }
    }
    Pen {
        fg: from_ratatui_color(cell.fg),
        bg: from_ratatui_color(cell.bg),
        ul: if attrs.contains(Attrs::UNDERLINE) {
            from_ratatui_color(cell.underline_color)
        } else {
            Color::Default
        },
        attrs,
    }
}

/// Encode a sequence of changed cells: cursor moves, SGR changes and text.
///
/// The cursor is moved with CUP at the start of every run of cells, and in
/// `robust` mode also after any character that isn't [`is_width_safe`], so a
/// terminal that draws such a character with a different width can't shift
/// the rest of the row.
pub fn encode_cells<'a>(
    out: &mut Vec<u8>,
    content: impl Iterator<Item = (u16, u16, &'a RCell)>,
    depth: ColorDepth,
    robust: bool,
) {
    // The terminal's pen is unknown at the start of a frame.
    out.extend_from_slice(b"\x1b[0m");
    let mut pen = Pen::RESET;
    let mut cursor: Option<(u16, u16)> = None;
    for (x, y, cell) in content {
        if cursor != Some((x, y)) {
            let _ = write!(out, "\x1b[{};{}H", y as u32 + 1, x as u32 + 1);
        }
        let sym = cell.symbol();
        let sym = if sym.is_empty() { " " } else { sym };
        let mut chars = sym.chars();
        let to = match (chars.next(), chars.next()) {
            (Some(c), None) => crate::sgr::needed_pen(c, pen_of(cell), &pen),
            _ => pen_of(cell),
        };
        transition(out, &pen, &to, depth);
        pen = to;
        out.extend_from_slice(sym.as_bytes());
        let width = sym.width().max(1) as u16;
        let risky = robust && sym.chars().any(|c| !is_width_safe(c));
        cursor = if risky { None } else { Some((x + width, y)) };
    }
    transition(out, &pen, &Pen::RESET, depth);
}

/// A ratatui backend writing to `W` (usually stdout).
pub struct TermBackend<W: Write> {
    out: W,
    buf: Vec<u8>,
    pub depth: ColorDepth,
    pub robust: bool,
    last_frame_bytes: usize,
}

impl<W: Write> TermBackend<W> {
    pub fn new(out: W, depth: ColorDepth, robust: bool) -> Self {
        TermBackend {
            out,
            buf: Vec::with_capacity(64 * 1024),
            depth,
            robust,
            last_frame_bytes: 0,
        }
    }

    /// Bytes sent by the last `draw` call.
    pub fn last_frame_bytes(&self) -> usize {
        self.last_frame_bytes
    }

    pub fn writer_mut(&mut self) -> &mut W {
        &mut self.out
    }
}

impl<W: Write> Backend for TermBackend<W> {
    type Error = io::Error;

    fn draw<'a, I>(&mut self, content: I) -> io::Result<()>
    where
        I: Iterator<Item = (u16, u16, &'a RCell)>,
    {
        self.buf.clear();
        encode_cells(&mut self.buf, content, self.depth, self.robust);
        self.last_frame_bytes = self.buf.len();
        self.out.write_all(&self.buf)
    }

    fn hide_cursor(&mut self) -> io::Result<()> {
        queue!(self.out, cursor::Hide)
    }

    fn show_cursor(&mut self) -> io::Result<()> {
        queue!(self.out, cursor::Show)
    }

    fn get_cursor_position(&mut self) -> io::Result<Position> {
        cursor::position()
            .map(|(x, y)| Position { x, y })
            .map_err(io::Error::other)
    }

    fn set_cursor_position<P: Into<Position>>(&mut self, position: P) -> io::Result<()> {
        let p = position.into();
        queue!(self.out, cursor::MoveTo(p.x, p.y))
    }

    fn clear(&mut self) -> io::Result<()> {
        queue!(self.out, terminal::Clear(terminal::ClearType::All))
    }

    fn clear_region(&mut self, clear_type: ClearType) -> io::Result<()> {
        let t = match clear_type {
            ClearType::All => terminal::ClearType::All,
            ClearType::AfterCursor => terminal::ClearType::FromCursorDown,
            ClearType::BeforeCursor => terminal::ClearType::FromCursorUp,
            ClearType::CurrentLine => terminal::ClearType::CurrentLine,
            ClearType::UntilNewLine => terminal::ClearType::UntilNewLine,
        };
        queue!(self.out, terminal::Clear(t))
    }

    fn size(&self) -> io::Result<Size> {
        let (w, h) = terminal::size()?;
        Ok(Size::new(w, h))
    }

    fn window_size(&mut self) -> io::Result<WindowSize> {
        let ws = terminal::window_size()?;
        Ok(WindowSize {
            columns_rows: Size::new(ws.columns, ws.rows),
            pixels: Size::new(ws.width, ws.height),
        })
    }

    fn flush(&mut self) -> io::Result<()> {
        self.out.flush()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use ratatui::buffer::Buffer;
    use ratatui::layout::Rect;

    fn encode(buf: &Buffer, robust: bool) -> String {
        let prev = Buffer::empty(buf.area);
        let mut out = Vec::new();
        encode_cells(&mut out, prev.diff(buf).into_iter(), ColorDepth::True, robust);
        String::from_utf8(out).unwrap().replace('\x1b', "ESC")
    }

    #[test]
    fn runs_start_with_cup() {
        let mut b = Buffer::empty(Rect::new(0, 0, 6, 2));
        b.set_string(1, 0, "ab", ratatui::style::Style::default());
        b.set_string(4, 1, "c", ratatui::style::Style::default());
        assert_eq!(encode(&b, false), "ESC[0mESC[1;2HabESC[2;5Hc");
    }

    #[test]
    fn robust_mode_repositions_after_risky_chars() {
        let mut b = Buffer::empty(Rect::new(0, 0, 4, 1));
        b.set_string(0, 0, "\u{1CD00}█x", ratatui::style::Style::default());
        assert_eq!(encode(&b, true), "ESC[0mESC[1;1H\u{1CD00}ESC[1;2H█x");
        assert_eq!(encode(&b, false), "ESC[0mESC[1;1H\u{1CD00}█x");
    }
}
