//! The text layer: real terminal characters at their page positions.

use std::collections::HashMap;

use sshpdf_doc::{PageText, RectPt, Rgb, TextLine, TextWord, char_flags};
use unicode_width::UnicodeWidthChar;

use crate::mapping::Mapping;

/// One character placed on the page grid.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct TextCell {
    pub ch: char,
    /// A combining mark on `ch`, or `'\0'`.
    pub mark: char,
    /// [`char_flags`] of the character.
    pub flags: u8,
    pub color: Rgb,
    /// Takes two columns (CJK).
    pub wide: bool,
    /// Index of the character it shows, counting the characters of all
    /// lines of the page in [`PageText::lines`] order (see [`char_index`]).
    pub src: u32,
}

/// The text layer of a page at one zoom.
#[derive(Clone, Debug, Default)]
pub struct TextLayer {
    /// Characters by (col, row) of the page grid. A wide character is stored
    /// at its left column only.
    pub cells: HashMap<(i32, i32), TextCell>,
    /// Lines that couldn't be placed legibly (left to the raster layer), as
    /// the band around their baseline where their glyph runs start (scripts
    /// included, neighbouring lines not), with the row they would have had.
    pub dropped: Vec<(RectPt, i32)>,
}

impl TextLayer {
    /// Whether every line in rows `r0..r1` was placed.
    pub fn legible_in(&self, r0: i32, r1: i32) -> bool {
        !self.dropped.iter().any(|(_, r)| *r >= r0 && *r < r1)
    }
}

/// Unicode superscript for a character, where one exists.
pub fn superscript(c: char) -> Option<char> {
    Some(match c {
        '0' => '⁰',
        '1' => '¹',
        '2' => '²',
        '3' => '³',
        '4' => '⁴',
        '5' => '⁵',
        '6' => '⁶',
        '7' => '⁷',
        '8' => '⁸',
        '9' => '⁹',
        '+' => '⁺',
        '-' | '−' => '⁻',
        '=' => '⁼',
        '(' => '⁽',
        ')' => '⁾',
        'n' => 'ⁿ',
        'i' => 'ⁱ',
        _ => return None,
    })
}

/// Unicode subscript for a character, where one exists.
pub fn subscript(c: char) -> Option<char> {
    Some(match c {
        '0' => '₀',
        '1' => '₁',
        '2' => '₂',
        '3' => '₃',
        '4' => '₄',
        '5' => '₅',
        '6' => '₆',
        '7' => '₇',
        '8' => '₈',
        '9' => '₉',
        '+' => '₊',
        '-' | '−' => '₋',
        '=' => '₌',
        '(' => '₍',
        ')' => '₎',
        'a' => 'ₐ',
        'e' => 'ₑ',
        'o' => 'ₒ',
        'x' => 'ₓ',
        _ => return None,
    })
}

/// Index of the first character of each line, counting the characters of
/// all lines of the page in order.
pub fn char_index(text: &PageText) -> Vec<u32> {
    let mut base = 0u32;
    text.lines()
        .map(|l| {
            let b = base;
            base += l.char_count() as u32;
            b
        })
        .collect()
}

/// The cells of a word: scripts turned into Unicode super/subscripts, or
/// `^`/`_` before a run that has no Unicode form; combining marks attached.
/// `src` is the page-wide index of the word's first character.
pub fn word_cells(word: &TextWord) -> Vec<TextCell> {
    word_cells_from(word, 0)
}

/// [`word_cells`] with source indices starting at `src`.
pub fn word_cells_from(word: &TextWord, src: u32) -> Vec<TextCell> {
    let mut out: Vec<TextCell> = Vec::with_capacity(word.chars.len());
    let chars = &word.chars;
    let mut i = 0;
    while i < chars.len() {
        let c = chars[i];
        let script = c.flags & (char_flags::SUPERSCRIPT | char_flags::SUBSCRIPT);
        if script != 0 {
            let mut j = i;
            while j < chars.len() && chars[j].flags & script != 0 {
                j += 1;
            }
            let map = if script & char_flags::SUPERSCRIPT != 0 {
                superscript
            } else {
                subscript
            };
            let mapped: Option<Vec<char>> = chars[i..j].iter().map(|tc| map(tc.c)).collect();
            match mapped {
                Some(m) => {
                    for (k, ch) in m.into_iter().enumerate() {
                        push_char(&mut out, ch, &chars[i + k], src + (i + k) as u32);
                    }
                }
                None => {
                    let marker = if script & char_flags::SUPERSCRIPT != 0 {
                        '^'
                    } else {
                        '_'
                    };
                    push_char(&mut out, marker, &chars[i], src + i as u32);
                    for (k, tc) in chars[i..j].iter().enumerate() {
                        push_char(&mut out, tc.c, tc, src + (i + k) as u32);
                    }
                }
            }
            i = j;
        } else {
            push_char(&mut out, c.c, &c, src + i as u32);
            i += 1;
        }
    }
    out
}

fn push_char(out: &mut Vec<TextCell>, ch: char, tc: &sshpdf_doc::TextChar, src: u32) {
    match ch.width() {
        Some(0) | None => {
            if let Some(last) = out.last_mut()
                && last.mark == '\0'
            {
                last.mark = ch;
            }
        }
        Some(w) => out.push(TextCell {
            ch,
            mark: '\0',
            flags: tc.flags,
            color: tc.color,
            wide: w == 2,
            src,
        }),
    }
}

fn cells_width(cells: &[TextCell]) -> i32 {
    cells.iter().map(|c| if c.wide { 2 } else { 1 }).sum()
}

/// Where a line's glyph runs start: along its baseline, from a little
/// above (superscripts) to a little below (subscripts).
fn baseline_band(line: &TextLine) -> RectPt {
    let s = line.size;
    RectPt::new(
        line.bbox.x0 - 0.5,
        line.baseline - 0.55 * s,
        line.bbox.x1 + 0.5,
        line.baseline + 0.3 * s,
    )
}

/// The row a line sits on: from the middle of its lowercase letters.
pub fn line_row(line: &TextLine, m: &Mapping) -> (i32, f32) {
    let r = m.row_of(line.baseline - 0.33 * line.size);
    (r.floor() as i32, r - r.floor())
}

struct Prepared<'a> {
    line: &'a TextLine,
    words: Vec<(i32, Vec<TextCell>)>,
    row: i32,
    frac: f32,
    chars: usize,
}

/// Lay out a line's words on one row. Returns the start column of each word,
/// or `None` if the line needs more room than its own width plus some
/// overflow (the zoom is too low for it).
fn fit_words(p: &Prepared<'_>, m: &Mapping) -> Option<Vec<i32>> {
    let n = p.words.len();
    let widths: Vec<i32> = p.words.iter().map(|(_, c)| cells_width(c)).collect();
    let mut pos: Vec<i32> = Vec::with_capacity(n);
    for i in 0..n {
        let ideal = p.words[i].0;
        pos.push(if i == 0 {
            ideal
        } else {
            ideal.max(pos[i - 1] + widths[i - 1] + 1)
        });
    }
    let right = m.col_of(p.line.bbox.x1).round() as i32;
    let start = pos[0];
    if pos[n - 1] + widths[n - 1] > right {
        // Take room from the gaps between words, keeping one space.
        let mut limit = right;
        for i in (0..n).rev() {
            pos[i] = pos[i].min(limit - widths[i]);
            limit = pos[i] - 1;
        }
        pos[0] = pos[0].max(start);
        for i in 1..n {
            pos[i] = pos[i].max(pos[i - 1] + widths[i - 1] + 1);
        }
    }
    let end = pos[n - 1] + widths[n - 1];
    let span = (right - start).max(1);
    let wide = p.words.iter().flat_map(|(_, c)| c).filter(|c| c.wide).count();
    let allowance = if wide * 3 > p.chars {
        (0.6 * span as f32).ceil() as i32
    } else {
        ((0.15 * span as f32).ceil() as i32).max(2)
    };
    (end - right <= allowance).then_some(pos)
}

/// Place a page's text on the grid.
pub fn text_layer(text: &PageText, m: &Mapping) -> TextLayer {
    let bases = char_index(text);
    let mut lines: Vec<Prepared<'_>> = text
        .lines()
        .zip(bases)
        .filter(|(l, _)| !l.words.is_empty())
        .map(|(l, base)| {
            let (row, frac) = line_row(l, m);
            let mut src = base;
            Prepared {
                line: l,
                words: l
                    .words
                    .iter()
                    .map(|w| {
                        let cells = word_cells_from(w, src);
                        src += w.chars.len() as u32;
                        (m.col_of(w.bbox.x0).round() as i32, cells)
                    })
                    .filter(|(_, c)| !c.is_empty())
                    .collect(),
                row,
                frac,
                chars: l.char_count(),
            }
        })
        .filter(|p| !p.words.is_empty())
        .collect();
    // Top to bottom; on the same row, longer lines first (they keep the
    // row when two lines collide).
    lines.sort_by(|a, b| a.row.cmp(&b.row).then(b.chars.cmp(&a.chars)));

    let mut layer = TextLayer::default();
    let mut taken: HashMap<i32, Vec<(i32, i32)>> = HashMap::new();
    let free = |taken: &HashMap<i32, Vec<(i32, i32)>>, row: i32, a: i32, b: i32| {
        taken.get(&row).is_none_or(|v| v.iter().all(|&(s, e)| b < s || a > e))
    };
    for p in &lines {
        let Some(pos) = fit_words(p, m) else {
            layer.dropped.push((baseline_band(p.line), p.row));
            continue;
        };
        let start = pos[0];
        let end = pos[pos.len() - 1] + cells_width(&p.words[p.words.len() - 1].1);
        let order = if p.frac < 0.5 {
            [p.row, p.row - 1, p.row + 1]
        } else {
            [p.row, p.row + 1, p.row - 1]
        };
        let Some(row) = order.into_iter().find(|&r| free(&taken, r, start, end)) else {
            layer.dropped.push((baseline_band(p.line), p.row));
            continue;
        };
        taken.entry(row).or_default().push((start, end));
        for ((_, cells), &col) in p.words.iter().zip(&pos) {
            let mut c = col;
            for cell in cells {
                layer.cells.insert((c, row), *cell);
                c += if cell.wide { 2 } else { 1 };
            }
        }
    }
    layer
}

#[cfg(test)]
mod tests {
    use super::*;
    use sshpdf_doc::{TextBlock, TextChar};

    fn word(s: &str, x: f32, y: f32, size: f32, adv: f32) -> TextWord {
        let chars: Vec<TextChar> = s
            .chars()
            .enumerate()
            .map(|(i, c)| TextChar {
                c,
                bbox: RectPt::new(
                    x + adv * i as f32,
                    y - 0.8 * size,
                    x + adv * (i + 1) as f32,
                    y + 0.2 * size,
                ),
                baseline: y,
                size,
                color: [0, 0, 0],
                font: 0,
                flags: 0,
            })
            .collect();
        let bbox = chars.iter().fold(chars[0].bbox, |b, c| b.union(&c.bbox));
        TextWord { chars, bbox }
    }

    fn line(words: Vec<TextWord>) -> TextLine {
        let bbox = words.iter().fold(words[0].bbox, |b, w| b.union(&w.bbox));
        TextLine {
            baseline: words[0].chars[0].baseline,
            size: words[0].chars[0].size,
            bbox,
            words,
        }
    }

    fn page(lines: Vec<TextLine>) -> PageText {
        PageText {
            blocks: vec![TextBlock {
                bbox: lines.iter().fold(lines[0].bbox, |b, l| b.union(&l.bbox)),
                lines,
            }],
            ..PageText::default()
        }
    }

    fn row_text(l: &TextLayer, row: i32, cols: i32) -> String {
        (0..cols)
            .map(|c| l.cells.get(&(c, row)).map_or(' ', |t| t.ch))
            .collect::<String>()
            .trim_end()
            .to_string()
    }

    #[test]
    fn words_land_at_their_columns() {
        let m = Mapping::new(12.0, 0.5); // 6 pt columns
        let t = page(vec![line(vec![
            word("Hello", 12.0, 20.0, 10.0, 5.0),
            word("world", 48.0, 20.0, 10.0, 5.0),
        ])]);
        let l = text_layer(&t, &m);
        // Row of y = 20 - 3.3 = 16.7 -> row 1; columns 2 and 8.
        assert_eq!(row_text(&l, 1, 20), "  Hello world");
        assert!(l.dropped.is_empty());
    }

    #[test]
    fn overlapping_words_keep_a_space() {
        let m = Mapping::new(12.0, 0.5);
        // Wide letters: "abcdef" is 36 pt; the next word starts at 40 pt (col 7)
        // but the first ends at col 8.
        let t = page(vec![line(vec![
            word("abcdefgh", 0.0, 20.0, 10.0, 5.0),
            word("xy", 41.0, 20.0, 10.0, 5.0),
        ])]);
        let l = text_layer(&t, &m);
        assert_eq!(row_text(&l, 1, 20), "abcdefgh xy");
    }

    #[test]
    fn colliding_lines_move_to_a_free_row_or_drop() {
        let m = Mapping::new(24.0, 0.5); // zoomed out: two text lines per row
        // 12 pt per character: one column each at this zoom.
        let t = page(vec![
            line(vec![word("first line here", 0.0, 20.0, 10.0, 12.0)]),
            line(vec![word("second", 0.0, 32.0, 10.0, 12.0)]),
            line(vec![word("third", 0.0, 44.0, 10.0, 12.0)]),
        ]);
        let l = text_layer(&t, &m);
        // "second" and "third" both fall on row 1; "third" moves down.
        assert_eq!(row_text(&l, 0, 40), "first line here");
        assert_eq!(row_text(&l, 1, 40), "second");
        assert_eq!(row_text(&l, 2, 40), "third");
        assert!(l.dropped.is_empty());

        // Text much wider than its columns at this zoom is dropped.
        let t = page(vec![line(vec![word("squeezed text", 0.0, 20.0, 10.0, 3.0)])]);
        let l = text_layer(&t, &m);
        assert_eq!(l.dropped.len(), 1);
        assert!(!l.legible_in(0, 1));
    }

    #[test]
    fn scripts() {
        let mut w = word("x2", 0.0, 20.0, 10.0, 5.0);
        w.chars[1].flags |= char_flags::SUPERSCRIPT;
        let cells = word_cells(&w);
        assert_eq!(cells.iter().map(|c| c.ch).collect::<String>(), "x²");
        let mut w = word("xab", 0.0, 20.0, 10.0, 5.0);
        w.chars[1].flags |= char_flags::SUPERSCRIPT;
        w.chars[2].flags |= char_flags::SUPERSCRIPT;
        assert_eq!(word_cells(&w).iter().map(|c| c.ch).collect::<String>(), "x^ab");
        let mut w = word("H2O", 0.0, 20.0, 10.0, 5.0);
        w.chars[1].flags |= char_flags::SUBSCRIPT;
        assert_eq!(word_cells(&w).iter().map(|c| c.ch).collect::<String>(), "H₂O");
    }

    #[test]
    fn wide_characters_take_two_columns() {
        let m = Mapping::new(12.0, 0.5);
        let t = page(vec![line(vec![word("漢字", 0.0, 20.0, 10.0, 10.0)])]);
        let l = text_layer(&t, &m);
        assert!(l.cells.get(&(0, 1)).unwrap().wide);
        assert_eq!(l.cells.get(&(2, 1)).unwrap().ch, '字');
    }
}
