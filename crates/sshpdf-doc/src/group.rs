//! Grouping glyphs into words, lines and blocks, and putting the blocks in
//! reading order. Backend-independent; backends feed it [`RawGlyph`]s in
//! content-stream order.

use crate::{RectPt, Rgb, TextBlock, TextChar, TextLine, TextWord, char_flags};

/// A glyph as a backend reports it, in page space.
#[derive(Clone, Debug, PartialEq)]
pub struct RawGlyph {
    /// Unicode text; may be several characters (ligatures), empty if unmapped.
    pub text: String,
    /// Origin of the glyph (on the baseline).
    pub x: f32,
    pub y: f32,
    /// Advance width in points.
    pub adv: f32,
    /// Font size in points.
    pub size: f32,
    pub color: Rgb,
    pub font: u16,
    pub flags: u8,
}

include!(concat!(env!("OUT_DIR"), "/radicals.rs"));

/// The ideograph a CJK radical look-alike stands for (⽂ → 文).
pub fn unradical(c: char) -> char {
    if !('\u{2E80}'..='\u{2FDF}').contains(&c) {
        return c;
    }
    match RADICALS.binary_search_by_key(&c, |(r, _)| *r) {
        Ok(i) => RADICALS[i].1,
        Err(_) => c,
    }
}

/// Expand a typographic ligature into its letters.
pub fn expand_ligature(c: char) -> Option<&'static str> {
    Some(match c {
        '\u{FB00}' => "ff",
        '\u{FB01}' => "fi",
        '\u{FB02}' => "fl",
        '\u{FB03}' => "ffi",
        '\u{FB04}' => "ffl",
        '\u{FB05}' | '\u{FB06}' => "st",
        '\u{0132}' => "IJ",
        '\u{0133}' => "ij",
        _ => return None,
    })
}

/// Characters that can't be shown: private use, controls, replacement.
fn unshowable(c: char) -> bool {
    matches!(c, '\u{E000}'..='\u{F8FF}' | '\u{F0000}'..='\u{10FFFF}' | '\u{FFFD}') || (c.is_control() && c != '\t')
}

/// A character during grouping, with explicit spaces kept as separators.
#[derive(Clone, Copy, Debug)]
struct Ch {
    tc: TextChar,
    space: bool,
}

/// Turn raw glyphs into characters: expand ligatures, split multi-character
/// glyphs over the advance, map unshowable glyphs to `?`. Returns the
/// characters and the number of unmapped glyphs.
fn to_chars(glyphs: &[RawGlyph]) -> (Vec<Ch>, usize) {
    let mut out = Vec::with_capacity(glyphs.len());
    let mut unmapped = 0;
    for g in glyphs {
        let mut text: String = String::new();
        if g.text.is_empty() {
            text.push('?');
            unmapped += 1;
        } else {
            for c in g.text.chars() {
                if let Some(s) = expand_ligature(c) {
                    text.push_str(s);
                } else if unshowable(c) {
                    text.push('?');
                    unmapped += 1;
                } else {
                    text.push(unradical(c));
                }
            }
        }
        let n = text.chars().count().max(1) as f32;
        let w = g.adv.max(0.0) / n;
        let asc = g.size * 0.8;
        let desc = g.size * 0.22;
        for (i, c) in text.chars().enumerate() {
            let x0 = g.x + w * i as f32;
            let space = c.is_whitespace();
            out.push(Ch {
                tc: TextChar {
                    c: if space { ' ' } else { c },
                    bbox: RectPt::new(x0, g.y - asc, x0 + w, g.y + desc),
                    baseline: g.y,
                    size: g.size,
                    color: g.color,
                    font: g.font,
                    flags: g.flags,
                },
                space,
            });
        }
    }
    (out, unmapped)
}

/// A line under construction.
#[derive(Clone, Debug)]
struct LineC {
    chars: Vec<Ch>,
    baseline: f32,
    size: f32,
    x0: f32,
    x1: f32,
}

impl LineC {
    fn new(chars: Vec<Ch>) -> LineC {
        let mut sizes: Vec<f32> = chars.iter().filter(|c| !c.space).map(|c| c.tc.size).collect();
        if sizes.is_empty() {
            sizes = chars.iter().map(|c| c.tc.size).collect();
        }
        sizes.sort_by(f32::total_cmp);
        let size = sizes[sizes.len() / 2];
        let mut bl: Vec<f32> = chars.iter().map(|c| c.tc.baseline).collect();
        bl.sort_by(f32::total_cmp);
        let baseline = bl[bl.len() / 2];
        let x0 = chars.iter().map(|c| c.tc.bbox.x0).fold(f32::MAX, f32::min);
        let x1 = chars.iter().map(|c| c.tc.bbox.x1).fold(f32::MIN, f32::max);
        LineC {
            chars,
            baseline,
            size,
            x0,
            x1,
        }
    }

    fn visible_len(&self) -> usize {
        self.chars.iter().filter(|c| !c.space).count()
    }
}

/// Remove characters drawn twice at (almost) the same place, as some PDFs
/// do to fake bold.
fn dedupe(chars: &mut Vec<Ch>) {
    chars.sort_by(|a, b| {
        a.tc.baseline
            .total_cmp(&b.tc.baseline)
            .then(a.tc.bbox.x0.total_cmp(&b.tc.bbox.x0))
    });
    let mut keep = vec![true; chars.len()];
    for i in 0..chars.len() {
        if !keep[i] {
            continue;
        }
        let a = chars[i].tc;
        for j in i + 1..chars.len() {
            let b = chars[j].tc;
            if b.baseline - a.baseline > 0.06 * a.size {
                break;
            }
            if keep[j] && b.c == a.c && (b.bbox.x0 - a.bbox.x0).abs() < 0.1 * a.size {
                keep[j] = false;
            }
        }
    }
    let mut k = keep.iter();
    chars.retain(|_| *k.next().expect("same length"));
}

/// Cluster characters into lines: by baseline, then split at wide gaps
/// (column gutters, table cells).
fn build_lines(mut chars: Vec<Ch>) -> Vec<LineC> {
    dedupe(&mut chars);
    // Sorted by baseline (dedupe sorted them).
    let mut clusters: Vec<Vec<Ch>> = Vec::new();
    let mut cur: Vec<Ch> = Vec::new();
    let mut rep = 0.0f32;
    for ch in chars {
        if !cur.is_empty() && (ch.tc.baseline - rep).abs() > 0.2 * ch.tc.size.max(1.0) {
            clusters.push(std::mem::take(&mut cur));
        }
        if cur.is_empty() {
            rep = ch.tc.baseline;
        }
        cur.push(ch);
    }
    if !cur.is_empty() {
        clusters.push(cur);
    }
    let mut lines = Vec::new();
    for mut cl in clusters {
        cl.sort_by(|a, b| a.tc.bbox.x0.total_cmp(&b.tc.bbox.x0));
        let mut part: Vec<Ch> = Vec::new();
        let mut end = f32::MIN;
        for ch in cl {
            let gap = ch.tc.bbox.x0 - end;
            let size = ch.tc.size.max(part.last().map_or(0.0, |p: &Ch| p.tc.size));
            if !part.is_empty() && gap > 1.2 * size.max(1.0) {
                lines.push(LineC::new(std::mem::take(&mut part)));
            }
            end = if part.is_empty() {
                ch.tc.bbox.x1
            } else {
                end.max(ch.tc.bbox.x1)
            };
            part.push(ch);
        }
        if !part.is_empty() {
            lines.push(LineC::new(part));
        }
    }
    // Lines of only spaces are noise.
    lines.retain(|l| l.visible_len() > 0);
    lines
}

/// Attach small raised or lowered lines (super- and subscripts) to the line
/// they belong to.
fn attach_scripts(lines: &mut Vec<LineC>) {
    // Smallest first, so nested scripts attach to their direct host first.
    let mut order: Vec<usize> = (0..lines.len()).collect();
    order.sort_by(|&a, &b| lines[a].size.total_cmp(&lines[b].size));
    let mut absorbed = vec![false; lines.len()];
    for &s in &order {
        let small = &lines[s];
        let mut host: Option<(usize, f32)> = None;
        for (h, big) in lines.iter().enumerate() {
            if h == s || absorbed[h] || big.size < small.size / 0.85 {
                continue;
            }
            let d = small.baseline - big.baseline;
            if d < -0.65 * big.size || d > 0.4 * big.size {
                continue;
            }
            let margin = 0.35 * big.size;
            if small.x0 < big.x0 - margin || small.x0 > big.x1 + margin {
                continue;
            }
            if host.is_none_or(|(_, hd)| d.abs() < hd.abs()) {
                host = Some((h, d));
            }
        }
        if let Some((h, d)) = host {
            let flag = if d < -0.12 * lines[h].size {
                char_flags::SUPERSCRIPT
            } else if d > 0.08 * lines[h].size {
                char_flags::SUBSCRIPT
            } else {
                0
            };
            let (sx0, sx1) = (lines[s].x0, lines[s].x1);
            let mut moved = std::mem::take(&mut lines[s].chars);
            for c in &mut moved {
                c.tc.flags |= flag;
            }
            let host = &mut lines[h];
            host.chars.extend(moved);
            host.chars.sort_by(|a, b| a.tc.bbox.x0.total_cmp(&b.tc.bbox.x0));
            host.x0 = host.x0.min(sx0);
            host.x1 = host.x1.max(sx1);
            absorbed[s] = true;
        }
    }
    let mut a = absorbed.iter();
    lines.retain(|_| !*a.next().expect("same length"));
}

/// Split a line into words at explicit spaces and at gaps.
fn words_of(line: &LineC) -> Vec<TextWord> {
    let has_spaces = line.chars.iter().any(|c| c.space);
    let threshold = if has_spaces { 0.3 } else { 0.15 } * line.size;
    let mut words = Vec::new();
    let mut cur: Vec<TextChar> = Vec::new();
    let mut end = f32::MIN;
    let flush = |cur: &mut Vec<TextChar>, words: &mut Vec<TextWord>| {
        if !cur.is_empty() {
            let bbox = cur.iter().skip(1).fold(cur[0].bbox, |b, c| b.union(&c.bbox));
            words.push(TextWord {
                chars: std::mem::take(cur),
                bbox,
            });
        }
    };
    for ch in &line.chars {
        if ch.space {
            flush(&mut cur, &mut words);
            end = ch.tc.bbox.x1;
            continue;
        }
        if !cur.is_empty() && ch.tc.bbox.x0 - end > threshold {
            flush(&mut cur, &mut words);
        }
        end = if cur.is_empty() {
            ch.tc.bbox.x1
        } else {
            end.max(ch.tc.bbox.x1)
        };
        cur.push(ch.tc);
    }
    flush(&mut cur, &mut words);
    words
}

fn finish_line(l: LineC) -> Option<TextLine> {
    let words = words_of(&l);
    if words.is_empty() {
        return None;
    }
    let bbox = words.iter().skip(1).fold(words[0].bbox, |b, w| b.union(&w.bbox));
    Some(TextLine {
        words,
        bbox,
        baseline: l.baseline,
        size: l.size,
    })
}

/// Group lines into blocks (about paragraphs).
fn build_blocks(mut lines: Vec<TextLine>) -> Vec<TextBlock> {
    lines.sort_by(|a, b| a.baseline.total_cmp(&b.baseline).then(a.bbox.x0.total_cmp(&b.bbox.x0)));
    struct B {
        lines: Vec<TextLine>,
        bbox: RectPt,
        leading: Option<f32>,
    }
    let mut blocks: Vec<B> = Vec::new();
    for line in lines {
        let mut best: Option<(usize, f32)> = None;
        for (i, b) in blocks.iter().enumerate() {
            let prev = b.lines.last().expect("blocks are not empty");
            let d = line.baseline - prev.baseline;
            if d <= 0.3 * prev.size {
                continue;
            }
            let ratio = line.size / prev.size;
            if !(0.75..=1.33).contains(&ratio) {
                continue;
            }
            let max_d = match b.leading {
                Some(lead) => lead * 1.3,
                None => 1.9 * line.size.max(prev.size),
            };
            if d > max_d {
                continue;
            }
            // Must overlap horizontally with the block.
            let overlap = line.bbox.x1.min(b.bbox.x1) - line.bbox.x0.max(b.bbox.x0);
            if overlap <= 0.0 {
                continue;
            }
            // A short previous line followed by an indented one starts a new
            // paragraph.
            let em = prev.size;
            if b.lines.len() >= 2 && prev.bbox.x1 < b.bbox.x1 - 2.0 * em && line.bbox.x0 > b.bbox.x0 + 0.6 * em {
                continue;
            }
            if best.is_none_or(|(_, bd)| d < bd) {
                best = Some((i, d));
            }
        }
        match best {
            Some((i, d)) => {
                let b = &mut blocks[i];
                b.leading = Some(match b.leading {
                    Some(l) => l.min(d),
                    None => d,
                });
                b.bbox = b.bbox.union(&line.bbox);
                b.lines.push(line);
            }
            None => blocks.push(B {
                bbox: line.bbox,
                lines: vec![line],
                leading: None,
            }),
        }
    }
    blocks
        .into_iter()
        .map(|b| TextBlock {
            lines: b.lines,
            bbox: b.bbox,
        })
        .collect()
}

/// Gaps in the projection of `spans` (start, end), widest first.
fn projection_gaps(mut spans: Vec<(f32, f32)>, min_gap: f32) -> Vec<(f32, f32)> {
    spans.sort_by(|a, b| a.0.total_cmp(&b.0));
    let mut gaps = Vec::new();
    let mut end = f32::MIN;
    for (s, e) in spans {
        if end > f32::MIN && s - end >= min_gap {
            gaps.push((end, s));
        }
        end = end.max(e);
    }
    gaps.sort_by(|a, b| (b.1 - b.0).total_cmp(&(a.1 - a.0)));
    gaps
}

/// The widest vertical gap between blocks (a column gutter), if any.
fn gutter(blocks: &[TextBlock], idx: &[usize], min_gap: f32) -> Option<(f32, f32)> {
    let spans = idx.iter().map(|&i| (blocks[i].bbox.x0, blocks[i].bbox.x1)).collect();
    projection_gaps(spans, min_gap).first().copied()
}

/// Split blocks into columns at the widest vertical gap, if there is one.
fn vertical_split(blocks: &[TextBlock], idx: &[usize], min_gap: f32) -> Option<(Vec<usize>, Vec<usize>)> {
    let spans = idx.iter().map(|&i| (blocks[i].bbox.x0, blocks[i].bbox.x1)).collect();
    let (start, end) = *projection_gaps(spans, min_gap).first()?;
    let mid = (start + end) / 2.0;
    let (l, r): (Vec<usize>, Vec<usize>) = idx.iter().partition(|&&i| blocks[i].bbox.x1 <= mid);
    (!l.is_empty() && !r.is_empty()).then_some((l, r))
}

/// Split blocks into horizontal bands at every vertical gap, top to bottom.
fn horizontal_bands(blocks: &[TextBlock], idx: &[usize]) -> Vec<Vec<usize>> {
    let mut sorted = idx.to_vec();
    sorted.sort_by(|&a, &b| blocks[a].bbox.y0.total_cmp(&blocks[b].bbox.y0));
    let mut bands: Vec<Vec<usize>> = Vec::new();
    let mut end = f32::MIN;
    for i in sorted {
        let b = &blocks[i].bbox;
        if bands.is_empty() || b.y0 > end {
            bands.push(vec![i]);
        } else {
            bands.last_mut().expect("non-empty").push(i);
        }
        end = end.max(b.y1);
    }
    bands
}

/// Reading order by recursive XY-cut: columns first; where something spans
/// the columns, horizontal bands, merging neighbouring bands back while they
/// still split into the same columns (so paragraph gaps that happen to line
/// up across columns don't cut the page into strips).
fn reading_order(blocks: &[TextBlock], idx: Vec<usize>, min_gap: f32, out: &mut Vec<usize>) {
    if idx.len() <= 1 {
        out.extend(idx);
        return;
    }
    if let Some((l, r)) = vertical_split(blocks, &idx, min_gap) {
        reading_order(blocks, l, min_gap, out);
        reading_order(blocks, r, min_gap, out);
        return;
    }
    let bands = horizontal_bands(blocks, &idx);
    if bands.len() <= 1 {
        let mut idx = idx;
        idx.sort_by(|&a, &b| {
            let (a, b) = (&blocks[a].bbox, &blocks[b].bbox);
            a.y0.total_cmp(&b.y0).then(a.x0.total_cmp(&b.x0))
        });
        out.extend(idx);
        return;
    }
    let mut groups: Vec<Vec<usize>> = Vec::new();
    for band in bands {
        if let Some(last) = groups.last_mut() {
            let mut merged = last.clone();
            merged.extend(&band);
            // Keep a band with the columns above it when none of its blocks
            // crosses their gutter (it may be one block, like the end of a
            // column that runs longer than its neighbour; a page number in
            // the gutter is not part of the columns).
            let fits = gutter(blocks, last, min_gap).is_some_and(|(g0, g1)| {
                band.iter()
                    .all(|&i| blocks[i].bbox.x1 <= g0 + 1.0 || blocks[i].bbox.x0 >= g1 - 1.0)
            });
            if fits && vertical_split(blocks, &merged, min_gap).is_some() {
                *last = merged;
                continue;
            }
        }
        groups.push(band);
    }
    for g in groups {
        reading_order(blocks, g, min_gap, out);
    }
}

/// Group glyphs (in content-stream order) into blocks in reading order.
/// Returns the blocks and the number of unmapped glyphs.
pub fn group(glyphs: &[RawGlyph]) -> (Vec<TextBlock>, usize) {
    let (chars, unmapped) = to_chars(glyphs);
    if chars.is_empty() {
        return (Vec::new(), unmapped);
    }
    let mut lines = build_lines(chars);
    attach_scripts(&mut lines);
    let lines: Vec<TextLine> = lines.into_iter().filter_map(finish_line).collect();
    let mut sizes: Vec<f32> = lines.iter().map(|l| l.size).collect();
    sizes.sort_by(f32::total_cmp);
    let median = sizes.get(sizes.len() / 2).copied().unwrap_or(10.0);
    let blocks = build_blocks(lines);
    let mut order = Vec::with_capacity(blocks.len());
    reading_order(
        &blocks,
        (0..blocks.len()).collect(),
        (0.8 * median).max(6.0),
        &mut order,
    );
    let mut slots: Vec<Option<TextBlock>> = blocks.into_iter().map(Some).collect();
    let blocks = order.into_iter().filter_map(|i| slots[i].take()).collect();
    (blocks, unmapped)
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Glyphs for `text` at (x, y), each 0.5 em wide; spaces are gaps.
    fn run(text: &str, x: f32, y: f32, size: f32) -> Vec<RawGlyph> {
        let mut out = Vec::new();
        let mut cx = x;
        for c in text.chars() {
            if c != ' ' {
                out.push(RawGlyph {
                    text: c.to_string(),
                    x: cx,
                    y,
                    adv: size * 0.5,
                    size,
                    color: [0, 0, 0],
                    font: 0,
                    flags: 0,
                });
            }
            cx += size * 0.5;
        }
        out
    }

    fn texts(blocks: &[TextBlock]) -> Vec<String> {
        blocks
            .iter()
            .map(|b| b.lines.iter().map(TextLine::text).collect::<Vec<_>>().join(" / "))
            .collect()
    }

    #[test]
    fn words_and_lines() {
        let mut g = run("hello world", 10.0, 20.0, 10.0);
        g.extend(run("second line", 10.0, 32.0, 10.0));
        let (blocks, _) = group(&g);
        assert_eq!(texts(&blocks), vec!["hello world / second line"]);
        assert_eq!(blocks[0].lines[0].words.len(), 2);
    }

    #[test]
    fn columns_are_separate_lines_and_read_in_order() {
        let mut g = Vec::new();
        for i in 0..5 {
            let y = 100.0 + 12.0 * i as f32;
            g.extend(run(&format!("left {i}"), 50.0, y, 10.0));
            g.extend(run(&format!("right {i}"), 300.0, y, 10.0));
        }
        let (blocks, _) = group(&g);
        assert_eq!(
            texts(&blocks),
            vec![
                "left 0 / left 1 / left 2 / left 3 / left 4",
                "right 0 / right 1 / right 2 / right 3 / right 4"
            ]
        );
    }

    #[test]
    fn title_then_columns_then_footer() {
        let mut g = run("A Title Across The Page", 100.0, 50.0, 18.0);
        for i in 0..4 {
            let y = 100.0 + 12.0 * i as f32;
            g.extend(run("aaaa bbbb cccc dddd", 50.0, y, 10.0));
            g.extend(run("eeee ffff gggg hhhh", 300.0, y, 10.0));
        }
        // A paragraph gap at the same height in both columns.
        for i in 0..3 {
            let y = 170.0 + 12.0 * i as f32;
            g.extend(run("iiii jjjj kkkk llll", 50.0, y, 10.0));
            g.extend(run("mmmm nnnn oooo pppp", 300.0, y, 10.0));
        }
        g.extend(run("7", 260.0, 400.0, 10.0));
        let (blocks, _) = group(&g);
        let t = texts(&blocks);
        assert!(t[0].starts_with("A Title"), "{t:?}");
        let pos = |s: &str| t.iter().position(|x| x.starts_with(s)).unwrap();
        assert!(pos("aaaa") < pos("iiii"), "{t:?}");
        assert!(pos("iiii") < pos("eeee"), "{t:?}");
        assert!(pos("eeee") < pos("mmmm"), "{t:?}");
        assert_eq!(t.last().unwrap(), "7");
    }

    #[test]
    fn longer_left_column_ends_before_the_right_one_starts() {
        let mut g = run("A Title Across The Page Here", 100.0, 50.0, 18.0);
        for i in 0..4 {
            g.extend(run("left one text", 50.0, 100.0 + 12.0 * i as f32, 10.0));
            g.extend(run("right one text", 300.0, 100.0 + 12.0 * i as f32, 10.0));
        }
        // The left column goes on below the end of the right one.
        for i in 0..3 {
            g.extend(run("left two text", 50.0, 180.0 + 12.0 * i as f32, 10.0));
        }
        let (blocks, _) = group(&g);
        let t = texts(&blocks);
        let pos = |s: &str| t.iter().position(|x| x.starts_with(s)).unwrap();
        assert!(pos("left one") < pos("left two"), "{t:?}");
        assert!(pos("left two") < pos("right one"), "{t:?}");
    }

    #[test]
    fn superscript_attaches_to_its_line() {
        let mut g = run("x", 10.0, 50.0, 10.0);
        g.extend(run("2", 15.0, 46.0, 7.0));
        g.extend(run(" y", 18.5, 50.0, 10.0));
        let (blocks, _) = group(&g);
        assert_eq!(blocks.len(), 1);
        let line = &blocks[0].lines[0];
        let chars: Vec<&TextChar> = line.words.iter().flat_map(|w| &w.chars).collect();
        let two = chars.iter().find(|c| c.c == '2').unwrap();
        assert!(two.has(char_flags::SUPERSCRIPT));
        assert_eq!(line.text(), "x2 y");
    }

    #[test]
    fn ligatures_and_unmapped() {
        let g = vec![
            RawGlyph {
                text: "\u{FB01}".into(),
                x: 0.0,
                y: 10.0,
                adv: 10.0,
                size: 10.0,
                color: [0; 3],
                font: 0,
                flags: 0,
            },
            RawGlyph {
                text: String::new(),
                x: 10.0,
                y: 10.0,
                adv: 5.0,
                size: 10.0,
                color: [0; 3],
                font: 0,
                flags: 0,
            },
        ];
        let (blocks, unmapped) = group(&g);
        assert_eq!(blocks[0].lines[0].text(), "fi?");
        assert_eq!(unmapped, 1);
    }

    #[test]
    fn radicals_become_ideographs() {
        assert_eq!(unradical('\u{2F42}'), '文');
        assert_eq!(unradical('\u{2F00}'), '一');
        assert_eq!(unradical('a'), 'a');
        let g = run("x", 0.0, 10.0, 10.0);
        let mut g = g;
        g[0].text = "\u{2F42}".into();
        assert_eq!(group(&g).0[0].lines[0].text(), "文");
    }

    #[test]
    fn faked_bold_duplicates_are_removed() {
        let mut g = run("bold", 10.0, 50.0, 10.0);
        g.extend(run("bold", 10.3, 50.0, 10.0));
        let (blocks, _) = group(&g);
        assert_eq!(blocks[0].lines[0].text(), "bold");
    }

    #[test]
    fn paragraph_break_by_indent() {
        let mut g = run("aaaa aaaa aaaa aaaa aaaa", 10.0, 20.0, 10.0);
        g.extend(run("aaaa aaaa aaaa aaaa aaaa", 10.0, 32.0, 10.0));
        g.extend(run("short", 10.0, 44.0, 10.0));
        g.extend(run("bbbb bbbb bbbb bbbb", 25.0, 56.0, 10.0));
        let (blocks, _) = group(&g);
        assert_eq!(blocks.len(), 2, "{:?}", texts(&blocks));
    }
}
