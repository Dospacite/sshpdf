//! Outline, links and destinations.

use std::collections::HashSet;

use hayro::hayro_syntax::object::dict::keys::{
    ANNOTS, DEST, DESTS, FIRST, KIDS, LIMITS, NAMES, NEXT, OUTLINES, RECT, SUBTYPE, TITLE, URI,
};
use hayro::hayro_syntax::object::{Array, Dict, MaybeRef, Name, Object, ObjectIdentifier, Rect, String as PdfString};
use hayro::hayro_syntax::page::Page;
use hayro::kurbo::Point;

use super::HayroDoc;
use crate::{Dest, Link, LinkTarget, OutlineItem, RectPt};

/// PDFDocEncoding for bytes 0x80-0x9F (the rest matches Latin-1).
const PDFDOC_80: [char; 32] = [
    '•', '†', '‡', '…', '—', '–', 'ƒ', '⁄', '‹', '›', '−', '‰', '„', '“', '”', '‘', '’', '‚', '™', 'ﬁ', 'ﬂ', 'Ł', 'Œ',
    'Š', 'Ÿ', 'Ž', 'ı', 'ł', 'œ', 'š', 'ž', '\u{FFFD}',
];

/// Decode a PDF text string (UTF-16 with BOM, UTF-8 with BOM, or
/// PDFDocEncoding).
pub(crate) fn decode_text_string(b: &[u8]) -> String {
    let s = if b.starts_with(&[0xFE, 0xFF]) || b.starts_with(&[0xFF, 0xFE]) {
        let be = b[0] == 0xFE;
        let units: Vec<u16> = b[2..]
            .as_chunks::<2>()
            .0
            .iter()
            .map(|&c| {
                if be {
                    u16::from_be_bytes(c)
                } else {
                    u16::from_le_bytes(c)
                }
            })
            .collect();
        String::from_utf16_lossy(&units)
    } else if let Some(rest) = b.strip_prefix(&[0xEF, 0xBB, 0xBF]) {
        String::from_utf8_lossy(rest).into_owned()
    } else {
        b.iter()
            .map(|&c| match c {
                0x80..=0x9F => PDFDOC_80[(c - 0x80) as usize],
                _ => c as char,
            })
            .collect()
    };
    // Outline titles sometimes carry control characters (line breaks).
    s.chars().map(|c| if c.is_control() { ' ' } else { c }).collect()
}

fn catalog(doc: &HayroDoc) -> Option<Dict<'_>> {
    let xref = doc.pdf.xref();
    xref.get::<Dict<'_>>(xref.root_id())
}

/// Look up `key` in a name tree.
fn name_tree_lookup<'a>(node: &Dict<'a>, key: &[u8], depth: u32) -> Option<Object<'a>> {
    if depth > 32 {
        return None;
    }
    if let Some(names) = node.get::<Array<'a>>(NAMES) {
        let mut it = names.iter::<Object<'a>>();
        while let Some(k) = it.next() {
            let v = it.next()?;
            if let Object::String(s) = &k
                && s.as_bytes() == key
            {
                return Some(v);
            }
        }
    }
    if let Some(kids) = node.get::<Array<'a>>(KIDS) {
        for kid in kids.iter::<Dict<'a>>() {
            if let Some(limits) = kid.get::<Array<'a>>(LIMITS) {
                let l: Vec<PdfString<'_>> = limits.iter::<PdfString<'_>>().collect();
                if l.len() == 2 && (key < l[0].as_bytes() || key > l[1].as_bytes()) {
                    continue;
                }
            }
            if let Some(v) = name_tree_lookup(&kid, key, depth + 1) {
                return Some(v);
            }
        }
    }
    None
}

/// Resolve a named destination to its destination object.
fn named_dest<'a>(doc: &'a HayroDoc, name: &[u8]) -> Option<Object<'a>> {
    let cat = catalog(doc)?;
    // PDF 1.2+: name tree in /Names /Dests.
    if let Some(names) = cat.get::<Dict<'a>>(NAMES)
        && let Some(tree) = names.get::<Dict<'a>>(DESTS)
        && let Some(v) = name_tree_lookup(&tree, name, 0)
    {
        return Some(v);
    }
    // PDF 1.1: a /Dests dictionary.
    let dests = cat.get::<Dict<'a>>(DESTS)?;
    let name = std::str::from_utf8(name).ok()?;
    dests.get::<Object<'a>>(name.as_bytes())
}

/// Turn a destination (array, name, string, or a dict with /D) into a page
/// and position.
fn resolve_dest(doc: &HayroDoc, obj: Object<'_>, depth: u32) -> Option<Dest> {
    if depth > 8 {
        return None;
    }
    match obj {
        Object::Array(arr) => {
            let mut raw = arr.raw_iter();
            let page = match raw.next()? {
                MaybeRef::Ref(r) => *doc.page_ids.get(&ObjectIdentifier::from(r))?,
                MaybeRef::NotRef(Object::Number(n)) => n.as_i64().max(0) as usize,
                _ => return None,
            };
            let page = page.min(doc.page_count_internal().saturating_sub(1));
            let items: Vec<Object<'_>> = arr.iter::<Object<'_>>().collect();
            let kind = match items.get(1) {
                Some(Object::Name(n)) => n.as_str().to_string(),
                _ => String::new(),
            };
            let num = |i: usize| match items.get(i) {
                Some(Object::Number(n)) => Some(n.as_f64()),
                _ => None,
            };
            // The y coordinate (PDF user space) the view should start at.
            let top = match kind.as_str() {
                "XYZ" => num(3).map(|t| (num(2).unwrap_or(0.0), t)),
                "FitH" | "FitBH" => num(2).map(|t| (0.0, t)),
                "FitR" => num(5).map(|t| (num(2).unwrap_or(0.0), t)),
                _ => None,
            };
            let y = top.and_then(|(x, t)| {
                let p = doc.pdf.pages().get(page)?;
                let q = HayroDoc::page_transform(p) * Point::new(x, t);
                Some(q.y as f32)
            });
            Some(Dest { page, y })
        }
        Object::Name(n) => resolve_dest(doc, named_dest(doc, n.as_str().as_bytes())?, depth + 1),
        Object::String(s) => resolve_dest(doc, named_dest(doc, s.as_bytes())?, depth + 1),
        Object::Dict(d) => resolve_dest(doc, d.get::<Object<'_>>(b"D")?, depth + 1),
        _ => None,
    }
}

impl HayroDoc {
    fn page_count_internal(&self) -> usize {
        self.pdf.pages().len()
    }
}

/// What an outline item or link points to: /Dest or a GoTo/URI action.
fn target_of(doc: &HayroDoc, item: &Dict<'_>, page: usize) -> Option<LinkTarget> {
    if let Some(d) = item.get::<Object<'_>>(DEST) {
        return resolve_dest(doc, d, 0).map(LinkTarget::Internal);
    }
    let action = item.get::<Dict<'_>>(b"A")?;
    let kind = action.get::<Name<'_>>(b"S")?;
    match kind.as_str() {
        "GoTo" => resolve_dest(doc, action.get::<Object<'_>>(b"D")?, 0).map(LinkTarget::Internal),
        "URI" => {
            let uri = action.get::<PdfString<'_>>(URI)?;
            Some(LinkTarget::External(
                String::from_utf8_lossy(uri.as_bytes()).into_owned(),
            ))
        }
        "Named" => {
            let last = doc.page_count_internal().saturating_sub(1);
            let p = match action.get::<Name<'_>>(b"N")?.as_str() {
                "NextPage" => (page + 1).min(last),
                "PrevPage" => page.saturating_sub(1),
                "FirstPage" => 0,
                "LastPage" => last,
                _ => return None,
            };
            Some(LinkTarget::Internal(Dest { page: p, y: None }))
        }
        "GoToR" | "Launch" => {
            let f = action.get::<Object<'_>>(b"F")?;
            let name = match f {
                Object::String(s) => decode_text_string(s.as_bytes()),
                Object::Dict(d) => decode_text_string(d.get::<PdfString<'_>>(b"F")?.as_bytes()),
                _ => return None,
            };
            Some(LinkTarget::External(name))
        }
        _ => None,
    }
}

fn outline_items(
    doc: &HayroDoc,
    first: Option<Dict<'_>>,
    seen: &mut HashSet<ObjectIdentifier>,
    depth: u32,
) -> Vec<OutlineItem> {
    let mut out = Vec::new();
    if depth > 32 {
        return out;
    }
    let mut cur = first;
    while let Some(item) = cur {
        if let Some(id) = item.obj_id()
            && !seen.insert(id)
        {
            break;
        }
        let title = item
            .get::<PdfString<'_>>(TITLE)
            .map(|s| decode_text_string(s.as_bytes()))
            .unwrap_or_default();
        let dest = match target_of(doc, &item, 0) {
            Some(LinkTarget::Internal(d)) => Some(d),
            _ => None,
        };
        let children = outline_items(doc, item.get::<Dict<'_>>(FIRST), seen, depth + 1);
        out.push(OutlineItem {
            title: title.trim().to_string(),
            dest,
            children,
        });
        cur = item.get::<Dict<'_>>(NEXT);
    }
    out
}

pub(super) fn outline(doc: &HayroDoc) -> Vec<OutlineItem> {
    let Some(cat) = catalog(doc) else {
        return Vec::new();
    };
    let Some(outlines) = cat.get::<Dict<'_>>(OUTLINES) else {
        return Vec::new();
    };
    let mut seen = HashSet::new();
    outline_items(doc, outlines.get::<Dict<'_>>(FIRST), &mut seen, 0)
}

pub(super) fn links(doc: &HayroDoc, page: &Page<'_>, page_index: usize) -> Vec<Link> {
    let Some(annots) = page.raw().get::<Array<'_>>(ANNOTS) else {
        return Vec::new();
    };
    let t = HayroDoc::page_transform(page);
    let mut out = Vec::new();
    for annot in annots.iter::<Dict<'_>>() {
        if annot.get::<Name<'_>>(SUBTYPE).is_none_or(|s| s.as_str() != "Link") {
            continue;
        }
        let Some(r) = annot.get::<Rect>(RECT) else {
            continue;
        };
        let a = t * Point::new(r.x0, r.y0);
        let b = t * Point::new(r.x1, r.y1);
        let rect = RectPt::new(a.x as f32, a.y as f32, b.x as f32, b.y as f32);
        if let Some(target) = target_of(doc, &annot, page_index) {
            out.push(Link { rect, target });
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::decode_text_string;

    #[test]
    fn text_strings() {
        assert_eq!(decode_text_string(b"Intro"), "Intro");
        assert_eq!(decode_text_string(&[0xFE, 0xFF, 0x00, 0x41, 0x4E, 0x2D]), "A中");
        assert_eq!(decode_text_string(&[0x8D, b'x', 0x8E]), "\u{201C}x\u{201D}");
        assert_eq!(decode_text_string(&[0x93]), "\u{FB01}");
        assert_eq!(decode_text_string(b"a\nb"), "a b");
    }
}
