//! SGR (Select Graphic Rendition) escape sequences.

use std::io::{self, Write};

use crate::caps::ColorDepth;
use crate::cell::{Attrs, Cell, Color};
use crate::palette;

/// The graphic state of the terminal: colors and attributes.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Pen {
    pub fg: Color,
    pub bg: Color,
    pub ul: Color,
    pub attrs: Attrs,
}

impl Pen {
    /// The state after SGR 0.
    pub const RESET: Pen = Pen {
        fg: Color::Default,
        bg: Color::Default,
        ul: Color::Default,
        attrs: Attrs::NONE,
    };

    pub fn of(cell: &Cell) -> Pen {
        Pen {
            fg: cell.fg,
            bg: cell.bg,
            ul: if cell.attrs.contains(Attrs::UNDERLINE) {
                cell.ul
            } else {
                Color::Default
            },
            attrs: cell.attrs,
        }
    }
}

/// The pen a cell needs, given the current one: a blank cell doesn't show
/// its foreground and a full block doesn't show its background, so those
/// keep whatever the terminal has (fewer bytes, especially in photos).
pub fn needed_pen(sym: char, wanted: Pen, current: &Pen) -> Pen {
    let plain = !wanted.attrs.contains(Attrs::REVERSE) && !wanted.attrs.contains(Attrs::UNDERLINE);
    let mut p = wanted;
    if plain && sym == ' ' {
        p.fg = current.fg;
    } else if plain && sym == '\u{2588}' {
        p.bg = current.bg;
    }
    p
}

/// Convert a color to what a terminal of the given depth can show.
pub fn fit_color(c: Color, depth: ColorDepth) -> Color {
    match (c, depth) {
        (Color::Rgb(r, g, b), ColorDepth::Ansi256) => Color::Indexed(palette::nearest([r, g, b], 256)),
        (Color::Rgb(r, g, b), ColorDepth::Ansi16) => Color::Indexed(palette::nearest([r, g, b], 16)),
        (Color::Indexed(i), ColorDepth::Ansi16) if i >= 16 => {
            Color::Indexed(palette::nearest(palette::xterm_rgb(i), 16))
        }
        (c, _) => c,
    }
}

fn push_num(out: &mut Vec<u8>, n: u32) {
    let _ = write!(out, "{n}");
}

/// Kind of color parameter: foreground, background or underline.
#[derive(Clone, Copy)]
enum Slot {
    Fg,
    Bg,
    Ul,
}

fn push_color(out: &mut Vec<u8>, slot: Slot, c: Color) {
    if !out.is_empty() && *out.last().expect("non-empty") != b'[' {
        out.push(b';');
    }
    match (slot, c) {
        (Slot::Fg, Color::Default) => out.extend_from_slice(b"39"),
        (Slot::Bg, Color::Default) => out.extend_from_slice(b"49"),
        (Slot::Ul, Color::Default) => out.extend_from_slice(b"59"),
        (Slot::Fg, Color::Indexed(i)) if i < 8 => push_num(out, 30 + i as u32),
        (Slot::Fg, Color::Indexed(i)) if i < 16 => push_num(out, 90 + (i - 8) as u32),
        (Slot::Bg, Color::Indexed(i)) if i < 8 => push_num(out, 40 + i as u32),
        (Slot::Bg, Color::Indexed(i)) if i < 16 => push_num(out, 100 + (i - 8) as u32),
        (_, Color::Indexed(i)) => {
            let base = match slot {
                Slot::Fg => b"38;5;",
                Slot::Bg => b"48;5;",
                Slot::Ul => b"58;5;",
            };
            out.extend_from_slice(base);
            push_num(out, i as u32);
        }
        (_, Color::Rgb(r, g, b)) => {
            let base = match slot {
                Slot::Fg => b"38;2;",
                Slot::Bg => b"48;2;",
                Slot::Ul => b"58;2;",
            };
            out.extend_from_slice(base);
            let _ = write!(out, "{r};{g};{b}");
        }
    }
}

fn push_param(out: &mut Vec<u8>, p: &[u8]) {
    if *out.last().expect("starts with CSI") != b'[' {
        out.push(b';');
    }
    out.extend_from_slice(p);
}

/// Append the shortest SGR sequence that changes the terminal from `from`
/// to `to`. Appends nothing if they are equal.
pub fn transition(out: &mut Vec<u8>, from: &Pen, to: &Pen, depth: ColorDepth) {
    let to = Pen {
        fg: fit_color(to.fg, depth),
        bg: fit_color(to.bg, depth),
        ul: fit_color(to.ul, depth),
        attrs: to.attrs,
    };
    if *from == to {
        return;
    }
    let start = out.len();
    out.extend_from_slice(b"\x1b[");
    let mut from = *from;
    // Turning attributes off one by one costs more than a reset when
    // several go at once; a reset also clears the colors.
    let removed = from.attrs.bits() & !to.attrs.bits();
    if removed.count_ones() > 1 || (to == Pen::RESET) {
        push_param(out, b"0");
        from = Pen::RESET;
    } else if removed != 0 {
        let r = Attrs::from_bits(removed);
        if r.contains(Attrs::BOLD) || r.contains(Attrs::DIM) {
            push_param(out, b"22");
            // 22 clears both bold and dim.
            from.attrs.remove(Attrs::BOLD);
            from.attrs.remove(Attrs::DIM);
        }
        if r.contains(Attrs::ITALIC) {
            push_param(out, b"23");
            from.attrs.remove(Attrs::ITALIC);
        }
        if r.contains(Attrs::UNDERLINE) {
            push_param(out, b"24");
            from.attrs.remove(Attrs::UNDERLINE);
        }
        if r.contains(Attrs::REVERSE) {
            push_param(out, b"27");
            from.attrs.remove(Attrs::REVERSE);
        }
    }
    let added = to.attrs.bits() & !from.attrs.bits();
    let a = Attrs::from_bits(added);
    if a.contains(Attrs::BOLD) {
        push_param(out, b"1");
    }
    if a.contains(Attrs::DIM) {
        push_param(out, b"2");
    }
    if a.contains(Attrs::ITALIC) {
        push_param(out, b"3");
    }
    if a.contains(Attrs::UNDERLINE) {
        push_param(out, b"4");
    }
    if a.contains(Attrs::REVERSE) {
        push_param(out, b"7");
    }
    if to.fg != from.fg {
        push_color(out, Slot::Fg, to.fg);
    }
    if to.bg != from.bg {
        push_color(out, Slot::Bg, to.bg);
    }
    if to.ul != from.ul {
        push_color(out, Slot::Ul, to.ul);
    }
    if out.len() == start + 2 {
        out.truncate(start);
    } else {
        out.push(b'm');
    }
}

/// Write the cells of a grid as lines of text with SGR colors, one line per
/// row, for output into a pipe or file (no cursor movement).
///
/// With `robust`, a CHA (cursor to column) sequence follows every character
/// that terminals might disagree about the width of, so one such character
/// can't shift the rest of the row.
pub fn write_grid(w: &mut impl Write, grid: &crate::cell::CellGrid, depth: ColorDepth, robust: bool) -> io::Result<()> {
    let mut out = Vec::with_capacity(grid.cols() as usize * grid.rows() as usize * 4);
    let mut buf = [0u8; 8];
    for r in 0..grid.rows() {
        let mut pen = Pen::RESET;
        let mut col = 0u16;
        for cell in grid.row(r) {
            if cell.wide == crate::cell::Wide::Trail {
                col += 1;
                continue;
            }
            let want = needed_pen(cell.ch, Pen::of(cell), &pen);
            transition(&mut out, &pen, &want, depth);
            pen = want;
            let n = cell.symbol(&mut buf);
            out.extend_from_slice(&buf[..n]);
            col += if cell.wide == crate::cell::Wide::Lead { 2 } else { 1 };
            if robust && !crate::backend::is_width_safe(cell.ch) && col < grid.cols() {
                let _ = write!(out, "\x1b[{}G", col + 1);
            }
        }
        transition(&mut out, &pen, &Pen::RESET, depth);
        out.push(b'\n');
    }
    w.write_all(&out)
}

impl Attrs {
    pub(crate) fn from_bits(b: u8) -> Attrs {
        let mut a = Attrs::NONE;
        for (bit, flag) in [
            (1, Attrs::BOLD),
            (2, Attrs::ITALIC),
            (4, Attrs::UNDERLINE),
            (8, Attrs::REVERSE),
            (16, Attrs::DIM),
        ] {
            if b & bit != 0 {
                a |= flag;
            }
        }
        a
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn tr(from: Pen, to: Pen, depth: ColorDepth) -> String {
        let mut v = Vec::new();
        transition(&mut v, &from, &to, depth);
        String::from_utf8(v).unwrap().replace('\x1b', "ESC")
    }

    #[test]
    fn same_pen_sends_nothing() {
        assert_eq!(tr(Pen::RESET, Pen::RESET, ColorDepth::True), "");
    }

    #[test]
    fn colors_and_attrs_in_one_sequence() {
        let to = Pen {
            fg: Color::Rgb(1, 2, 3),
            bg: Color::Indexed(4),
            ul: Color::Default,
            attrs: Attrs::BOLD,
        };
        assert_eq!(tr(Pen::RESET, to, ColorDepth::True), "ESC[1;38;2;1;2;3;44m");
    }

    #[test]
    fn removing_one_attr_uses_its_off_code() {
        let from = Pen {
            attrs: Attrs::BOLD | Attrs::ITALIC,
            ..Pen::RESET
        };
        let to = Pen {
            attrs: Attrs::ITALIC,
            ..Pen::RESET
        };
        assert_eq!(tr(from, to, ColorDepth::True), "ESC[22m");
    }

    #[test]
    fn removing_several_attrs_resets() {
        let from = Pen {
            fg: Color::Indexed(1),
            attrs: Attrs::BOLD | Attrs::ITALIC,
            ..Pen::RESET
        };
        let to = Pen {
            fg: Color::Indexed(1),
            ..Pen::RESET
        };
        assert_eq!(tr(from, to, ColorDepth::True), "ESC[0;31m");
    }

    #[test]
    fn rgb_is_reduced_for_lower_depths() {
        let to = Pen {
            fg: Color::Rgb(255, 0, 0),
            ..Pen::RESET
        };
        assert_eq!(tr(Pen::RESET, to, ColorDepth::Ansi256), "ESC[38;5;196m");
        assert_eq!(tr(Pen::RESET, to, ColorDepth::Ansi16), "ESC[91m");
    }

    #[test]
    fn grid_lines_end_with_reset() {
        let mut g = crate::cell::CellGrid::new(2, 1);
        g.set(0, 0, Cell::new('a', Color::Indexed(1), Color::Default));
        let mut out = Vec::new();
        write_grid(&mut out, &g, ColorDepth::True, false).unwrap();
        // The space doesn't show its foreground, so the red stays until the end.
        assert_eq!(String::from_utf8(out).unwrap(), "\x1b[31ma \x1b[0m\n");
    }

    #[test]
    fn blanks_keep_the_foreground() {
        let mut g = crate::cell::CellGrid::new(3, 1);
        g.set(0, 0, Cell::new('a', Color::Indexed(1), Color::Indexed(2)));
        g.set(1, 0, Cell::new(' ', Color::Indexed(3), Color::Indexed(4)));
        g.set(2, 0, Cell::new('█', Color::Indexed(5), Color::Indexed(6)));
        let mut out = Vec::new();
        write_grid(&mut out, &g, ColorDepth::True, false).unwrap();
        // The space changes only the background, the block only the foreground.
        assert_eq!(
            String::from_utf8(out).unwrap().replace('\x1b', "ESC"),
            "ESC[31;42maESC[44m ESC[35m█ESC[0m\n"
        );
    }
}
