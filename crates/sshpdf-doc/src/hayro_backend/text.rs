//! Text extraction with hayro: a device that records glyphs.

use std::collections::{HashMap, HashSet};

use hayro::hayro_interpret::font::{Glyph, GlyphRun};
use hayro::hayro_interpret::hayro_cmap::BfString;
use hayro::hayro_interpret::{
    BlendMode, ClipPath, Context, Device, DrawMode, DrawProps, Image, ImageDrawProps, InterpreterCache, Paint,
    SoftMask, interpret_page,
};
use hayro::hayro_syntax::object::dict::keys::{
    BASE_FONT, FIRST_CHAR, FLAGS, FONT_DESC, FONT_WEIGHT, ITALIC_ANGLE, RESOURCES, SUBTYPE, WIDTHS, XOBJECT,
};
use hayro::hayro_syntax::object::{Array, Dict, Name, ObjectIdentifier, Stream};
use hayro::hayro_syntax::page::Page;
use hayro::kurbo::{Affine, Rect};

use super::HayroDoc;
use crate::fontstyle::{flags as fd_flags, strip_subset, style_from_name};
use crate::group::{RawGlyph, group};
use crate::{FontInfo, PageText, RectPt, Rgb, TextChar, char_flags};

/// A simple font (Type1/Type3) found in the page resources, for telling
/// which font a glyph run uses by comparing glyph widths.
struct CandFont {
    info: FontInfo,
    first: u32,
    widths: Vec<f32>,
}

fn font_info_from_dict(dict: &Dict<'_>) -> FontInfo {
    let name = dict
        .get::<Name<'_>>(BASE_FONT)
        .map(|n| strip_subset(n.as_str()).to_string())
        .unwrap_or_default();
    let mut style = style_from_name(&name);
    if let Some(fd) = dict.get::<Dict<'_>>(FONT_DESC) {
        let flags = fd.get::<u32>(FLAGS).unwrap_or(0);
        if flags & fd_flags::ITALIC != 0 || fd.get::<f32>(ITALIC_ANGLE).is_some_and(|a| a.abs() > 1.0) {
            style.italic = true;
        }
        if flags & fd_flags::FORCE_BOLD != 0 || fd.get::<f32>(FONT_WEIGHT).is_some_and(|w| w >= 600.0) {
            style.bold = true;
        }
        if flags & fd_flags::FIXED_PITCH != 0 {
            style.mono = true;
        }
    }
    FontInfo {
        name,
        bold: style.bold,
        italic: style.italic,
        mono: style.mono,
    }
}

/// Collect the simple fonts of a resource dictionary and of the form
/// XObjects it uses.
fn collect_fonts(resources: &Dict<'_>, out: &mut Vec<CandFont>, seen: &mut HashSet<ObjectIdentifier>, depth: u32) {
    if depth > 6 {
        return;
    }
    if let Some(fonts) = resources.get::<Dict<'_>>(b"Font") {
        for key in fonts.keys() {
            let Some(font) = fonts.get::<Dict<'_>>(key.as_ref()) else {
                continue;
            };
            if let Some(id) = font.obj_id()
                && !seen.insert(id)
            {
                continue;
            }
            let subtype = font.get::<Name<'_>>(SUBTYPE);
            let simple = subtype
                .as_ref()
                .is_some_and(|s| matches!(s.as_str(), "Type1" | "MMType1" | "Type3" | "TrueType"));
            if !simple {
                continue;
            }
            let first = font.get::<u32>(FIRST_CHAR).unwrap_or(0);
            let widths: Vec<f32> = font
                .get::<Array<'_>>(WIDTHS)
                .map(|a| a.iter::<f32>().collect())
                .unwrap_or_default();
            out.push(CandFont {
                info: font_info_from_dict(&font),
                first,
                widths,
            });
        }
    }
    if let Some(xobjects) = resources.get::<Dict<'_>>(XOBJECT) {
        for key in xobjects.keys() {
            let Some(stream) = xobjects.get::<Stream<'_>>(key.as_ref()) else {
                continue;
            };
            if !seen.insert(stream.obj_id()) {
                continue;
            }
            let dict = stream.dict();
            if dict.get::<Name<'_>>(SUBTYPE).is_some_and(|s| s.as_str() == "Form")
                && let Some(res) = dict.get::<Dict<'_>>(RESOURCES)
            {
                collect_fonts(&res, out, seen, depth + 1);
            }
        }
    }
}

fn rgb_of(paint: &Paint<'_>) -> Rgb {
    match paint {
        Paint::Color(c) => {
            let [r, g, b, _] = c.to_rgba().to_rgba8();
            [r, g, b]
        }
        // Text filled with a pattern or shading: show it as plain ink.
        Paint::Pattern(_) => [0, 0, 0],
    }
}

fn bf_to_string(bf: BfString) -> String {
    match bf {
        BfString::Char(c) => c.to_string(),
        BfString::String(s) => s,
    }
}

/// Is the transform (glyph space → page space) horizontal, upright text?
pub(super) fn is_horizontal(t: &Affine) -> bool {
    let [a, b, _c, d, _, _] = t.as_coeffs();
    a > 0.0 && d < 0.0 && b.abs() <= 0.035 * a
}

struct TextDevice<'f> {
    glyphs: Vec<RawGlyph>,
    other: Vec<TextChar>,
    fonts: Vec<FontInfo>,
    font_index: HashMap<String, u16>,
    /// Fonts by their program (TrueType/CID fonts).
    program_fonts: HashMap<u128, u16>,
    cands: &'f [CandFont],
    last_cand: Option<usize>,
    /// The last fill run, to skip its stroke pass (text render mode 2).
    last_fill: Option<(Affine, usize)>,
}

impl TextDevice<'_> {
    fn intern(&mut self, info: FontInfo) -> u16 {
        let key = format!("{}|{}{}{}", info.name, info.bold, info.italic, info.mono);
        if let Some(&i) = self.font_index.get(&key) {
            return i;
        }
        let i = self.fonts.len() as u16;
        self.fonts.push(info);
        self.font_index.insert(key, i);
        i
    }

    /// Which page font a run of simple-font glyphs uses, by widths.
    fn match_simple_font(&mut self, codes: &[(u32, f32)]) -> Option<usize> {
        let mut best: Option<(usize, usize)> = None;
        for (i, f) in self.cands.iter().enumerate() {
            if f.widths.is_empty() {
                continue;
            }
            let mut hits = 0;
            let mut misses = 0;
            for &(code, w) in codes {
                if let Some(fw) = code.checked_sub(f.first).and_then(|k| f.widths.get(k as usize)) {
                    if (fw - w).abs() < 0.05 {
                        hits += 1;
                    } else {
                        misses += 1;
                    }
                }
            }
            if misses > 0 || hits == 0 {
                continue;
            }
            let better = match best {
                None => true,
                Some((bi, bh)) => hits > bh || (hits == bh && Some(i) == self.last_cand && Some(bi) != self.last_cand),
            };
            if better {
                best = Some((i, hits));
            }
        }
        if best.is_none() && self.cands.len() == 1 {
            return Some(0);
        }
        best.map(|(i, _)| i)
    }
}

impl<'a> Device<'a> for TextDevice<'_> {
    fn draw_path(&mut self, _: &hayro::kurbo::BezPath, _: DrawProps<'a>, _: &DrawMode) {}
    fn push_clip_path(&mut self, _: &ClipPath) {}
    fn push_transparency_group(&mut self, _: f32, _: Option<SoftMask<'a>>, _: BlendMode) {}
    fn draw_image(&mut self, _: Image<'a, '_>, _: ImageDrawProps<'a>) {}
    fn pop_clip(&mut self) {}
    fn pop_transparency_group(&mut self) {}

    fn draw_glyph_run(&mut self, run: &GlyphRun<'_, 'a>, props: DrawProps<'a>, mode: &DrawMode) {
        let glyphs = run.glyphs();
        let Some(first) = glyphs.first() else {
            return;
        };
        let key = (props.transform * first.transform(), glyphs.len());
        match mode {
            DrawMode::Stroke(_) if self.last_fill == Some(key) => return,
            DrawMode::Fill(_) | DrawMode::FillAndStroke(..) => self.last_fill = Some(key),
            _ => self.last_fill = None,
        }

        // Font of the run.
        let mut font: Option<u16> = None;
        let mut simple_codes = Vec::new();
        for g in glyphs {
            match &**g {
                Glyph::Outline(o) => match o.font_data() {
                    Some(d) if font.is_none() => {
                        font = Some(match self.program_fonts.get(&d.cache_key) {
                            Some(&i) => i,
                            None => {
                                let name = strip_subset(d.postscript_name.as_deref().unwrap_or("")).to_string();
                                let st = style_from_name(&name);
                                let info = FontInfo {
                                    bold: st.bold || d.weight.is_some_and(|w| w >= 600),
                                    italic: st.italic || d.is_italic,
                                    mono: st.mono || d.is_monospace,
                                    name,
                                };
                                let i = self.intern(info);
                                self.program_fonts.insert(d.cache_key, i);
                                i
                            }
                        });
                    }
                    Some(_) => {}
                    None => simple_codes.push((o.char_code(), o.advance_width().unwrap_or(-1.0))),
                },
                Glyph::Type3(t3) => simple_codes.push((t3.char_code(), t3.advance_width().unwrap_or(-1.0))),
            }
        }
        if font.is_none() {
            let m = self.match_simple_font(&simple_codes);
            if let Some(i) = m {
                self.last_cand = Some(i);
            }
            let info = m.map(|i| self.cands[i].info.clone()).unwrap_or_default();
            font = Some(self.intern(info));
        }
        let font = font.expect("set above");
        let info = &self.fonts[font as usize];
        let mut flags = 0;
        if info.bold {
            flags |= char_flags::BOLD;
        }
        if info.italic {
            flags |= char_flags::ITALIC;
        }
        if info.mono {
            flags |= char_flags::MONO;
        }
        if matches!(mode, DrawMode::Invisible) {
            flags |= char_flags::INVISIBLE;
        }
        let color = rgb_of(&props.paint);

        for g in glyphs {
            let t = props.transform * g.transform();
            let [a, b, c, d, e, f] = t.as_coeffs();
            let size = ((c * c + d * d).sqrt() * 1000.0) as f32;
            let adv_units = match &**g {
                Glyph::Outline(o) => o.advance_width(),
                Glyph::Type3(t3) => t3.advance_width(),
            }
            .unwrap_or(500.0) as f64;
            let mut adv = ((a * a + b * b).sqrt() * adv_units) as f32;
            if !(adv > 0.0 && adv < 4.0 * size) {
                adv = 0.5 * size;
            }
            let text = g.as_unicode().map(bf_to_string).unwrap_or_default();
            if is_horizontal(&t) {
                self.glyphs.push(RawGlyph {
                    text,
                    x: e as f32,
                    y: f as f32,
                    adv,
                    size,
                    color,
                    font,
                    flags,
                });
            } else {
                // Rotated or vertical text: keep for search, drawn by the raster layer.
                let p = |x: f64, y: f64| t * hayro::kurbo::Point::new(x, y);
                let corners = [p(0.0, -200.0), p(adv_units, -200.0), p(0.0, 800.0), p(adv_units, 800.0)];
                let r = corners
                    .iter()
                    .skip(1)
                    .fold(Rect::from_points(corners[0], corners[0]), |r, q| r.union_pt(*q));
                for ch in text.chars() {
                    self.other.push(TextChar {
                        c: ch,
                        bbox: RectPt::new(r.x0 as f32, r.y0 as f32, r.x1 as f32, r.y1 as f32),
                        baseline: f as f32,
                        size,
                        color,
                        font,
                        flags,
                    });
                }
            }
        }
    }
}

pub(super) fn extract(doc: &HayroDoc, page: &Page<'_>) -> PageText {
    let mut cands = Vec::new();
    let mut seen = HashSet::new();
    // Page resources (inherited ones included) as a raw dictionary.
    let res = page.resources();
    let mut res_dict_fonts = Vec::new();
    collect_page_fonts(res, &mut res_dict_fonts, &mut seen);
    cands.extend(res_dict_fonts);

    let mut dev = TextDevice {
        glyphs: Vec::new(),
        other: Vec::new(),
        fonts: Vec::new(),
        font_index: HashMap::new(),
        program_fonts: HashMap::new(),
        cands: &cands,
        last_cand: None,
        last_fill: None,
    };
    let (w, h) = page.render_dimensions();
    let cache = InterpreterCache::new();
    let mut ctx = Context::new(
        HayroDoc::page_transform(page),
        Rect::new(0.0, 0.0, w as f64, h as f64),
        &cache,
        page.xref(),
        doc.settings.clone(),
    );
    interpret_page(page, &mut ctx, &mut dev);
    let (blocks, unmapped) = group(&dev.glyphs);
    if unmapped > 0 {
        log::debug!("{unmapped} glyphs without a Unicode mapping");
    }
    PageText {
        blocks,
        fonts: dev.fonts,
        other: dev.other,
        unmapped,
    }
}

/// Fonts of the page resources and the forms they use.
fn collect_page_fonts(
    res: &hayro::hayro_syntax::page::Resources<'_>,
    out: &mut Vec<CandFont>,
    seen: &mut HashSet<ObjectIdentifier>,
) {
    // `Resources` splits the dictionary; rebuild what `collect_fonts` reads.
    for key in res.fonts.keys() {
        let Some(font) = res.fonts.get::<Dict<'_>>(key.as_ref()) else {
            continue;
        };
        if let Some(id) = font.obj_id()
            && !seen.insert(id)
        {
            continue;
        }
        let simple = font
            .get::<Name<'_>>(SUBTYPE)
            .is_some_and(|s| matches!(s.as_str(), "Type1" | "MMType1" | "Type3" | "TrueType"));
        if !simple {
            continue;
        }
        out.push(CandFont {
            info: font_info_from_dict(&font),
            first: font.get::<u32>(FIRST_CHAR).unwrap_or(0),
            widths: font
                .get::<Array<'_>>(WIDTHS)
                .map(|a| a.iter::<f32>().collect())
                .unwrap_or_default(),
        });
    }
    for key in res.x_objects.keys() {
        let Some(stream) = res.x_objects.get::<Stream<'_>>(key.as_ref()) else {
            continue;
        };
        if !seen.insert(stream.obj_id()) {
            continue;
        }
        let dict = stream.dict();
        if dict.get::<Name<'_>>(SUBTYPE).is_some_and(|s| s.as_str() == "Form")
            && let Some(r) = dict.get::<Dict<'_>>(RESOURCES)
        {
            collect_fonts(&r, out, seen, 1);
        }
    }
}
