//! Reflow: a page's text in reading order, wrapped to a width.
//!
//! Paragraphs come from the text blocks; a word split by a hyphen at a line
//! end is joined again; headings (much larger or bold text) get a blank line
//! before them; figures become placeholder lines.

use sshpdf_doc::{PageGraphics, PageText, RectPt, Rgb, TextBlock, char_flags};
use sshpdf_raster::Theme;
use sshpdf_raster::color::rgb_to_lab;
use sshpdf_term::{Attrs, Cell, CellGrid, Color};
use unicode_width::UnicodeWidthChar;

use crate::textlayer::{TextCell, word_cells_from};

/// What a reflowed line is.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum LineKind {
    Body,
    Heading,
    Blank,
    /// A placeholder for figure `index` (0-based, in reading order).
    Figure {
        index: usize,
    },
}

/// One output line.
#[derive(Clone, Debug, PartialEq)]
pub struct RLine {
    pub cells: Vec<TextCell>,
    pub kind: LineKind,
    /// Page the text comes from.
    pub page: usize,
    /// Where on the page the line's text starts (for jumping back to layout
    /// mode at the same place).
    pub y: f32,
}

impl RLine {
    pub fn plain(&self) -> String {
        let mut s = String::new();
        for c in &self.cells {
            s.push(c.ch);
            if c.mark != '\0' {
                s.push(c.mark);
            }
        }
        s
    }

    fn blank(page: usize, y: f32) -> RLine {
        RLine {
            cells: Vec::new(),
            kind: LineKind::Blank,
            page,
            y,
        }
    }
}

/// A figure on a page: an image or a cluster of vector graphics.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Figure {
    pub rect: RectPt,
}

/// Figures of a page: big enough images, in no particular order.
pub fn figures(graphics: &PageGraphics, page_area: f32) -> Vec<Figure> {
    let mut out: Vec<Figure> = Vec::new();
    for r in &graphics.images {
        if r.width() < 24.0 || r.height() < 24.0 || r.width() * r.height() < 0.005 * page_area {
            continue;
        }
        // Merge images that overlap (tiled images, masks).
        if let Some(f) = out.iter_mut().find(|f| f.rect.intersects(r)) {
            f.rect = f.rect.union(r);
        } else {
            out.push(Figure { rect: *r });
        }
    }
    out
}

/// Is this hyphen at a line end a break inside a word?
fn is_break_hyphen(c: char) -> bool {
    matches!(c, '-' | '\u{00AD}' | '\u{2010}')
}

/// Join the lines of a block into one stream of words. A word ending in a
/// hyphen at the end of a line, followed by a lowercase letter, is joined
/// with the next word without the hyphen.
pub fn block_words(block: &TextBlock) -> Vec<Vec<TextCell>> {
    block_words_from(block, 0)
}

/// [`block_words`] with source indices starting at `src` (the page-wide
/// index of the block's first character).
pub fn block_words_from(block: &TextBlock, mut src: u32) -> Vec<Vec<TextCell>> {
    let mut words: Vec<Vec<TextCell>> = Vec::new();
    let mut join_next = false;
    for line in &block.lines {
        for (wi, w) in line.words.iter().enumerate() {
            let cells = word_cells_from(w, src);
            src += w.chars.len() as u32;
            if cells.is_empty() {
                continue;
            }
            // CJK text has no spaces between words: a gap or line break
            // between two wide characters joins them directly.
            let cjk_join = words
                .last()
                .and_then(|w| w.last())
                .is_some_and(|c| c.wide && cells[0].wide);
            if wi == 0 && join_next {
                let starts_lower = cells[0].ch.is_lowercase();
                let prev = words.last_mut().expect("join needs a previous word");
                if starts_lower {
                    prev.pop();
                }
                prev.extend(cells);
            } else if cjk_join {
                words.last_mut().expect("checked").extend(cells);
            } else {
                words.push(cells);
            }
            join_next = false;
        }
        join_next = words
            .last()
            .and_then(|w| w.last())
            .is_some_and(|c| is_break_hyphen(c.ch))
            && words.last().is_some_and(|w| w.len() > 1);
    }
    words
}

fn width_of(cells: &[TextCell]) -> usize {
    cells.iter().map(|c| c.ch.width().unwrap_or(0)).sum()
}

/// Greedy word wrap.
fn wrap(words: Vec<Vec<TextCell>>, width: usize, kind: LineKind, page: usize, y: f32) -> Vec<RLine> {
    let width = width.max(8);
    let mut out = Vec::new();
    let mut cur: Vec<TextCell> = Vec::new();
    let mut cur_w = 0;
    let space = |flags: u8, color: Rgb| TextCell {
        ch: ' ',
        mark: '\0',
        flags,
        color,
        wide: false,
        src: u32::MAX,
    };
    let flush = |cur: &mut Vec<TextCell>, out: &mut Vec<RLine>| {
        out.push(RLine {
            cells: std::mem::take(cur),
            kind,
            page,
            y,
        });
    };
    for mut word in words {
        let mut ww = width_of(&word);
        // CJK may break between any two characters: fill the current line
        // instead of moving the whole run down.
        if word.first().is_some_and(|c| c.wide) && cur_w > 0 && cur_w + 1 + ww > width {
            let room = width.saturating_sub(cur_w + 1);
            let mut take = 0;
            let mut tw = 0;
            while take < word.len() && tw + word[take].ch.width().unwrap_or(0) <= room {
                tw += word[take].ch.width().unwrap_or(0);
                take += 1;
            }
            if take > 0 {
                let rest = word.split_off(take);
                let (f, c) = (word[0].flags & (char_flags::BOLD | char_flags::ITALIC), word[0].color);
                cur.push(space(f, c));
                cur.extend(word);
                flush(&mut cur, &mut out);
                cur_w = 0;
                word = rest;
                ww = width_of(&word);
                if word.is_empty() {
                    continue;
                }
            }
        }
        // A word longer than the line: split it.
        while ww > width {
            if cur_w > 0 {
                flush(&mut cur, &mut out);
                cur_w = 0;
            }
            let mut take = 0;
            let mut tw = 0;
            while take < word.len() && tw + word[take].ch.width().unwrap_or(0) <= width {
                tw += word[take].ch.width().unwrap_or(0);
                take += 1;
            }
            let rest = word.split_off(take.max(1));
            cur = word;
            flush(&mut cur, &mut out);
            word = rest;
            ww = width_of(&word);
        }
        if cur_w > 0 && cur_w + 1 + ww > width {
            flush(&mut cur, &mut out);
            cur_w = 0;
        }
        if cur_w > 0 {
            let (f, c) = (word[0].flags & (char_flags::BOLD | char_flags::ITALIC), word[0].color);
            cur.push(space(f, c));
            cur_w += 1;
        }
        cur_w += ww;
        cur.extend(word);
    }
    if !cur.is_empty() {
        flush(&mut cur, &mut out);
    }
    out
}

/// The main body font size of a page, by characters.
pub fn body_size(text: &PageText) -> f32 {
    let mut sizes: Vec<(i32, usize)> = Vec::new();
    for l in text.lines() {
        let k = (l.size * 2.0).round() as i32;
        match sizes.iter_mut().find(|(s, _)| *s == k) {
            Some((_, n)) => *n += l.char_count(),
            None => sizes.push((k, l.char_count())),
        }
    }
    sizes
        .iter()
        .max_by_key(|(_, n)| *n)
        .map_or(10.0, |(s, _)| *s as f32 / 2.0)
}

fn is_heading(block: &TextBlock, body: f32) -> bool {
    let chars: usize = block.lines.iter().map(|l| l.char_count()).sum();
    let size = block.lines.iter().map(|l| l.size).fold(0.0, f32::max);
    if block.lines.len() <= 3 && size >= 1.15 * body && chars < 200 {
        return true;
    }
    let all_bold = block
        .lines
        .iter()
        .flat_map(|l| &l.words)
        .flat_map(|w| &w.chars)
        .all(|c| c.has(char_flags::BOLD));
    all_bold && block.lines.len() <= 2 && chars < 100
}

/// Reflow one page to `width` columns.
pub fn reflow(text: &PageText, page: usize, width: usize, graphics: &PageGraphics) -> Vec<RLine> {
    let body = body_size(text);
    let blocks: Vec<&TextBlock> = text.blocks.iter().collect();
    let page_area = {
        let r = blocks
            .iter()
            .fold(RectPt::new(0.0, 0.0, 612.0, 792.0), |r, b| r.union(&b.bbox));
        r.width() * r.height()
    };
    let figs = figures(graphics, page_area);
    // Where each figure goes: before the first block (in reading order)
    // below it that overlaps it horizontally.
    let mut before: Vec<Vec<usize>> = vec![Vec::new(); blocks.len() + 1];
    let mut fig_order: Vec<(usize, usize)> = Vec::new();
    for (fi, f) in figs.iter().enumerate() {
        let slot = blocks
            .iter()
            .position(|b| b.bbox.y0 >= f.rect.y0 && b.bbox.x0 < f.rect.x1 && f.rect.x0 < b.bbox.x1)
            .unwrap_or(blocks.len());
        fig_order.push((slot, fi));
    }
    fig_order.sort();
    for (n, &(slot, _)) in fig_order.iter().enumerate() {
        before[slot].push(n);
    }

    let mut out: Vec<RLine> = Vec::new();
    let push_figure = |out: &mut Vec<RLine>, n: usize, y: f32| {
        if out.last().is_some_and(|l| l.kind != LineKind::Blank) {
            out.push(RLine::blank(page, y));
        }
        let label = format!("[figure p{} #{} — press Enter to view]", page + 1, n + 1);
        out.push(RLine {
            cells: label
                .chars()
                .map(|ch| TextCell {
                    ch,
                    mark: '\0',
                    flags: char_flags::ITALIC,
                    color: [90, 90, 90],
                    wide: ch.width() == Some(2),
                    src: u32::MAX,
                })
                .collect(),
            kind: LineKind::Figure { index: n },
            page,
            y,
        });
    };
    // A paragraph is held back until the next block shows whether it
    // continues it (a word hyphenated across a column or page break).
    struct Para {
        words: Vec<Vec<TextCell>>,
        kind: LineKind,
        y: f32,
    }
    let flush = |out: &mut Vec<RLine>, p: Option<Para>| {
        if let Some(p) = p {
            if out.last().is_some_and(|l| l.kind != LineKind::Blank) {
                out.push(RLine::blank(page, p.y));
            }
            out.extend(wrap(p.words, width, p.kind, page, p.y));
        }
    };
    let mut pending: Option<Para> = None;
    let mut base = 0u32;
    for (bi, block) in blocks.iter().enumerate() {
        if !before[bi].is_empty() {
            flush(&mut out, pending.take());
            for &n in &before[bi] {
                push_figure(&mut out, n, block.bbox.y0);
            }
        }
        let heading = is_heading(block, body);
        let mut words = block_words_from(block, base);
        base += block.lines.iter().map(|l| l.char_count() as u32).sum::<u32>();
        if words.is_empty() {
            continue;
        }
        if heading {
            for w in &mut words {
                for c in w.iter_mut() {
                    c.flags |= char_flags::BOLD;
                }
            }
        }
        let kind = if heading { LineKind::Heading } else { LineKind::Body };
        // Continues the held paragraph?
        if let Some(p) = pending.as_mut()
            && p.kind == LineKind::Body
            && kind == LineKind::Body
            && words[0].first().is_some_and(|c| c.ch.is_lowercase())
            && let Some(last) = p.words.last_mut()
            && last.len() > 1
            && last.last().is_some_and(|c| is_break_hyphen(c.ch))
        {
            last.pop();
            let mut rest = words.into_iter();
            last.extend(rest.next().expect("not empty"));
            p.words.extend(rest);
            continue;
        }
        flush(&mut out, pending.take());
        pending = Some(Para {
            words,
            kind,
            y: block.bbox.y0,
        });
    }
    flush(&mut out, pending.take());
    for &n in &before[blocks.len()] {
        push_figure(&mut out, n, f32::MAX);
    }
    out
}

/// Draw reflowed lines into a grid (top-left aligned).
pub fn to_grid(lines: &[RLine], cols: u16, rows: u16, theme: &Theme) -> CellGrid {
    let mut grid = CellGrid::new(cols, rows);
    for (r, line) in lines.iter().take(rows as usize).enumerate() {
        let mut c = 0u16;
        for tc in &line.cells {
            if c >= cols {
                break;
            }
            let mut attrs = Attrs::NONE;
            if tc.flags & char_flags::BOLD != 0 {
                attrs |= Attrs::BOLD;
            }
            if tc.flags & char_flags::ITALIC != 0 {
                attrs |= Attrs::ITALIC;
            }
            let fg = theme.text_color(rgb_to_lab(tc.color), None);
            let fg = if matches!(line.kind, LineKind::Figure { .. }) {
                Color::Indexed(8)
            } else {
                fg
            };
            let mut cell = Cell::new(tc.ch, fg, Color::Default).with_attrs(attrs);
            cell.mark = tc.mark;
            c += grid.set(c, r as u16, cell);
        }
    }
    grid
}

#[cfg(test)]
mod tests {
    use super::*;
    use sshpdf_doc::{TextChar, TextLine, TextWord};

    fn word(s: &str, x: f32, y: f32) -> TextWord {
        let chars: Vec<TextChar> = s
            .chars()
            .enumerate()
            .map(|(i, c)| TextChar {
                c,
                bbox: RectPt::new(x + 5.0 * i as f32, y - 8.0, x + 5.0 * (i + 1) as f32, y + 2.0),
                baseline: y,
                size: 10.0,
                color: [0, 0, 0],
                font: 0,
                flags: 0,
            })
            .collect();
        let bbox = chars.iter().fold(chars[0].bbox, |b, c| b.union(&c.bbox));
        TextWord { chars, bbox }
    }

    fn line(words: &[&str], y: f32) -> TextLine {
        let mut x = 0.0;
        let ws: Vec<TextWord> = words
            .iter()
            .map(|w| {
                let tw = word(w, x, y);
                x += 5.0 * (w.chars().count() as f32 + 1.0);
                tw
            })
            .collect();
        let bbox = ws.iter().fold(ws[0].bbox, |b, w| b.union(&w.bbox));
        TextLine {
            words: ws,
            bbox,
            baseline: y,
            size: 10.0,
        }
    }

    fn block(lines: Vec<TextLine>) -> TextBlock {
        let bbox = lines.iter().fold(lines[0].bbox, |b, l| b.union(&l.bbox));
        TextBlock { lines, bbox }
    }

    fn texts(words: &[Vec<TextCell>]) -> Vec<String> {
        words.iter().map(|w| w.iter().map(|c| c.ch).collect()).collect()
    }

    #[test]
    fn hyphen_at_line_end_rejoins() {
        let b = block(vec![line(&["a", "hyphen-"], 10.0), line(&["ated", "word"], 22.0)]);
        assert_eq!(texts(&block_words(&b)), vec!["a", "hyphenated", "word"]);
    }

    #[test]
    fn hyphen_before_capital_or_inside_line_stays() {
        let b = block(vec![
            line(&["well-known", "Hewlett-"], 10.0),
            line(&["Packard", "x"], 22.0),
        ]);
        assert_eq!(texts(&block_words(&b)), vec!["well-known", "Hewlett-Packard", "x"]);
        // A lone dash at a line end is punctuation, not a split word.
        let b = block(vec![line(&["yes", "-"], 10.0), line(&["no"], 22.0)]);
        assert_eq!(texts(&block_words(&b)), vec!["yes", "-", "no"]);
    }

    #[test]
    fn wraps_and_marks_headings() {
        let text = PageText {
            blocks: vec![
                block(vec![{
                    let mut l = line(&["Title"], 10.0);
                    l.size = 16.0;
                    l
                }]),
                block(vec![
                    line(&["one", "two", "three", "four", "five"], 30.0),
                    line(&["six"], 42.0),
                ]),
            ],
            ..PageText::default()
        };
        let lines = reflow(&text, 0, 10, &PageGraphics::default());
        let plain: Vec<String> = lines.iter().map(RLine::plain).collect();
        assert_eq!(plain, vec!["Title", "", "one two", "three four", "five six"]);
        assert_eq!(lines[0].kind, LineKind::Heading);
        assert!(lines[0].cells[0].flags & char_flags::BOLD != 0);
    }

    #[test]
    fn cjk_fills_lines() {
        let words = block_words(&block(vec![line(&["ab"], 10.0)]));
        let mut all = words;
        all.extend(block_words(&block(vec![line(&["漢字漢字漢字"], 10.0)])));
        let out = wrap(all, 8, LineKind::Body, 0, 0.0);
        let plain: Vec<String> = out.iter().map(RLine::plain).collect();
        assert_eq!(plain, vec!["ab 漢字", "漢字漢字"]);
    }

    #[test]
    fn cjk_lines_join_without_spaces() {
        let b = block(vec![line(&["漢字の", "文"], 10.0), line(&["続き"], 22.0)]);
        assert_eq!(texts(&block_words(&b)), vec!["漢字の文続き"]);
    }

    #[test]
    fn hyphen_across_blocks_rejoins() {
        let text = PageText {
            blocks: vec![
                block(vec![line(&["left", "column", "ends", "cor-"], 10.0)]),
                block(vec![line(&["rect", "reader"], 300.0)]),
            ],
            ..PageText::default()
        };
        let plain: Vec<String> = reflow(&text, 0, 60, &PageGraphics::default())
            .iter()
            .map(RLine::plain)
            .collect();
        assert_eq!(plain, vec!["left column ends correct reader"]);
    }

    #[test]
    fn long_words_are_split() {
        let words = vec![word("abcdefghijkl", 0.0, 10.0)];
        let b = block(vec![TextLine {
            bbox: words[0].bbox,
            words,
            baseline: 10.0,
            size: 10.0,
        }]);
        let out = wrap(block_words(&b), 8, LineKind::Body, 0, 0.0);
        let plain: Vec<String> = out.iter().map(RLine::plain).collect();
        assert_eq!(plain, vec!["abcdefgh", "ijkl"]);
    }

    #[test]
    fn figures_get_placeholders() {
        let text = PageText {
            blocks: vec![
                block(vec![line(&["above"], 10.0)]),
                block(vec![line(&["below"], 300.0)]),
            ],
            ..PageText::default()
        };
        let g = PageGraphics {
            paths: vec![],
            images: vec![RectPt::new(0.0, 50.0, 200.0, 250.0)],
        };
        let plain: Vec<String> = reflow(&text, 2, 60, &g).iter().map(RLine::plain).collect();
        assert_eq!(
            plain,
            vec!["above", "", "[figure p3 #1 — press Enter to view]", "", "below"]
        );
    }
}
