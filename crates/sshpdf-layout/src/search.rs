//! Finding text on a page.

use sshpdf_doc::{PageText, RectPt};

/// One match on a page.
#[derive(Clone, Debug, PartialEq)]
pub struct Match {
    /// Page-wide indices of the matched characters (see
    /// [`crate::textlayer::char_index`]).
    pub chars: Vec<u32>,
    /// Bounding box of the matched characters.
    pub bbox: RectPt,
}

fn fold(c: char) -> char {
    // Case-insensitive, and the common typographic variants match ASCII.
    match c {
        '‘' | '’' | '‚' | '′' => '\'',
        '“' | '”' | '„' | '″' => '"',
        '‐' | '‑' | '‒' | '–' | '—' | '−' => '-',
        '\u{a0}' => ' ',
        _ => c.to_lowercase().next().unwrap_or(c),
    }
}

/// Find `query` (case-insensitive) in each line of a page; words of a line
/// are joined with one space, and runs of spaces in the query count as one.
pub fn find(text: &PageText, query: &str) -> Vec<Match> {
    let q: Vec<char> = {
        let mut v: Vec<char> = Vec::new();
        for c in query.chars().map(fold) {
            if c == ' ' && v.last() == Some(&' ') {
                continue;
            }
            v.push(c);
        }
        while v.last() == Some(&' ') {
            v.pop();
        }
        while v.first() == Some(&' ') {
            v.remove(0);
        }
        v
    };
    if q.is_empty() {
        return Vec::new();
    }
    let mut out = Vec::new();
    let mut base = 0u32;
    for line in text.lines() {
        // The line as characters with their page-wide index (None for the
        // joining spaces) and boxes.
        let mut chars: Vec<(char, Option<u32>, RectPt)> = Vec::new();
        let mut idx = base;
        for (wi, w) in line.words.iter().enumerate() {
            if wi > 0 {
                chars.push((' ', None, w.bbox));
            }
            for c in &w.chars {
                chars.push((fold(c.c), Some(idx), c.bbox));
                idx += 1;
            }
        }
        base = idx;
        let mut i = 0;
        while i + q.len() <= chars.len() {
            if chars[i..i + q.len()].iter().map(|c| c.0).eq(q.iter().copied()) {
                let hit = &chars[i..i + q.len()];
                let bbox = hit
                    .iter()
                    .filter(|c| c.1.is_some())
                    .map(|c| c.2)
                    .reduce(|a, b| a.union(&b))
                    .unwrap_or(line.bbox);
                out.push(Match {
                    chars: hit.iter().filter_map(|c| c.1).collect(),
                    bbox,
                });
                i += q.len();
            } else {
                i += 1;
            }
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;
    use sshpdf_doc::{TextBlock, TextChar, TextLine, TextWord};

    fn page(lines: &[&str]) -> PageText {
        let mut ls = Vec::new();
        for (li, l) in lines.iter().enumerate() {
            let y = 10.0 + 12.0 * li as f32;
            let mut x = 0.0;
            let words = l
                .split(' ')
                .map(|w| {
                    let chars: Vec<TextChar> = w
                        .chars()
                        .map(|c| {
                            let tc = TextChar {
                                c,
                                bbox: RectPt::new(x, y - 8.0, x + 5.0, y + 2.0),
                                baseline: y,
                                size: 10.0,
                                color: [0; 3],
                                font: 0,
                                flags: 0,
                            };
                            x += 5.0;
                            tc
                        })
                        .collect();
                    x += 5.0;
                    let bbox = chars.iter().fold(chars[0].bbox, |b, c| b.union(&c.bbox));
                    TextWord { chars, bbox }
                })
                .collect::<Vec<_>>();
            let bbox = words.iter().fold(words[0].bbox, |b, w| b.union(&w.bbox));
            ls.push(TextLine {
                words,
                bbox,
                baseline: y,
                size: 10.0,
            });
        }
        let bbox = ls.iter().fold(ls[0].bbox, |b, l| b.union(&l.bbox));
        PageText {
            blocks: vec![TextBlock { lines: ls, bbox }],
            ..PageText::default()
        }
    }

    #[test]
    fn finds_across_words_case_insensitively() {
        let t = page(&["The Quick brown", "fox quick"]);
        let m = find(&t, "quick  BROWN");
        assert_eq!(m.len(), 1);
        // "The" is 0-2, "Quick" 3-7, "brown" 8-12.
        assert_eq!(m[0].chars, (3..13).collect::<Vec<u32>>());
        let m = find(&t, "quick");
        assert_eq!(m.len(), 2);
        assert_eq!(m[1].chars, vec![16, 17, 18, 19, 20]);
        assert!(find(&t, "  ").is_empty());
    }

    #[test]
    fn typographic_variants_match() {
        let t = page(&["it’s a well–known"]);
        assert_eq!(find(&t, "it's").len(), 1);
        assert_eq!(find(&t, "well-known").len(), 1);
    }
}
