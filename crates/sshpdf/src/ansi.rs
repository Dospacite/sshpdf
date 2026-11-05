//! Reading ANSI text (SGR colors, as `--dump` or `tmux capture-pane -e`
//! write it) back into a cell grid, to render captures as PNG.

use sshpdf_term::{Attrs, Cell, CellGrid, Color, char_width};

/// Parse lines of text with SGR escapes into a grid `cols` wide.
pub fn parse(text: &str, cols: u16) -> CellGrid {
    let lines: Vec<&str> = text.lines().collect();
    let mut grid = CellGrid::new(cols, lines.len().max(1) as u16);
    let mut pen = Cell::BLANK;
    for (r, line) in lines.iter().enumerate() {
        let mut c: u16 = 0;
        let mut chars = line.chars().peekable();
        while let Some(ch) = chars.next() {
            if ch == '\x1b' {
                match chars.peek() {
                    Some('[') => {
                        chars.next();
                        let mut params = String::new();
                        let mut fin = ' ';
                        for p in chars.by_ref() {
                            if ('@'..='~').contains(&p) {
                                fin = p;
                                break;
                            }
                            params.push(p);
                        }
                        match fin {
                            'm' => sgr(&params, &mut pen),
                            // Cursor to column (robust mode in dumps).
                            'G' => c = params.parse::<u16>().unwrap_or(1).saturating_sub(1),
                            _ => {}
                        }
                    }
                    Some(']') => {
                        // OSC: skip to BEL or ST.
                        while let Some(p) = chars.next() {
                            if p == '\x07' || (p == '\x1b' && chars.peek() == Some(&'\\')) {
                                break;
                            }
                        }
                    }
                    _ => {}
                }
                continue;
            }
            let w = char_width(ch);
            if w == 0 {
                if c > 0
                    && let Some(cell) = grid.get_mut(c - 1, r as u16)
                {
                    cell.mark = ch;
                }
                continue;
            }
            c += grid.set(c, r as u16, Cell { ch, mark: '\0', ..pen });
        }
    }
    grid
}

fn color_from(params: &mut std::slice::Iter<'_, u32>) -> Color {
    match params.next() {
        Some(5) => Color::Indexed(*params.next().unwrap_or(&0) as u8),
        Some(2) => {
            let r = *params.next().unwrap_or(&0) as u8;
            let g = *params.next().unwrap_or(&0) as u8;
            let b = *params.next().unwrap_or(&0) as u8;
            Color::Rgb(r, g, b)
        }
        _ => Color::Default,
    }
}

fn sgr(params: &str, pen: &mut Cell) {
    let nums: Vec<u32> = if params.is_empty() {
        vec![0]
    } else {
        params.split([';', ':']).map(|p| p.parse().unwrap_or(0)).collect()
    };
    let mut it = nums.iter();
    while let Some(&n) = it.next() {
        match n {
            0 => *pen = Cell::BLANK,
            1 => pen.attrs |= Attrs::BOLD,
            2 => pen.attrs |= Attrs::DIM,
            3 => pen.attrs |= Attrs::ITALIC,
            4 => pen.attrs |= Attrs::UNDERLINE,
            7 => pen.attrs |= Attrs::REVERSE,
            22 => {
                pen.attrs.remove(Attrs::BOLD);
                pen.attrs.remove(Attrs::DIM);
            }
            23 => pen.attrs.remove(Attrs::ITALIC),
            24 => pen.attrs.remove(Attrs::UNDERLINE),
            27 => pen.attrs.remove(Attrs::REVERSE),
            30..=37 => pen.fg = Color::Indexed((n - 30) as u8),
            38 => pen.fg = color_from(&mut it),
            39 => pen.fg = Color::Default,
            40..=47 => pen.bg = Color::Indexed((n - 40) as u8),
            48 => pen.bg = color_from(&mut it),
            49 => pen.bg = Color::Default,
            58 => pen.ul = color_from(&mut it),
            59 => pen.ul = Color::Default,
            90..=97 => pen.fg = Color::Indexed((n - 90 + 8) as u8),
            100..=107 => pen.bg = Color::Indexed((n - 100 + 8) as u8),
            _ => {}
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// What a cell looks like: a blank's foreground doesn't show.
    fn visible(c: &Cell) -> Cell {
        let mut c = *c;
        if c.ch == ' ' && !c.attrs.contains(Attrs::UNDERLINE) && !c.attrs.contains(Attrs::REVERSE) {
            c.fg = Color::Default;
        }
        c
    }

    #[test]
    fn round_trips_a_dump() {
        let mut g = CellGrid::new(4, 2);
        g.set(
            0,
            0,
            Cell::new('a', Color::Rgb(1, 2, 3), Color::Indexed(4)).with_attrs(Attrs::BOLD),
        );
        g.set(1, 1, Cell::new('漢', Color::Indexed(9), Color::Default));
        let mut out = Vec::new();
        sshpdf_term::sgr::write_grid(&mut out, &g, sshpdf_term::ColorDepth::True, false).unwrap();
        let back = parse(&String::from_utf8(out).unwrap(), 4);
        let a: Vec<Cell> = g.cells().iter().map(visible).collect();
        let b: Vec<Cell> = back.cells().iter().map(visible).collect();
        assert_eq!(a, b);
    }
}
