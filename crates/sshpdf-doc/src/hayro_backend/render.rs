//! Rendering a page region, leaving out what the text and line layers draw.

use std::borrow::Cow;

use hayro::hayro_interpret::font::GlyphRun;
use hayro::hayro_interpret::{DrawMode, DrawProps};
use hayro::hayro_syntax::page::Page;
use hayro::kurbo::{Affine, BezPath};
use hayro::vello_cpu::{self, Pixmap, RenderContext};
use hayro::{RenderCache, RenderFilter, RenderSettings, render_into_filtered};

use super::HayroDoc;
use super::graphics::{split_subpaths, subpath_in};
use super::text::is_horizontal;
use crate::{Exclusions, PointPt, RectPt, RgbaImage};

/// Tolerance (points) for matching a subpath against an excluded box.
const MATCH_TOL: f32 = 0.02;

struct Filter<'e> {
    ex: &'e Exclusions,
    /// Device pixels → page space.
    to_page: Affine,
    /// Excluded boxes sorted by `x0`, for a quick range lookup.
    sorted: Vec<RectPt>,
}

impl Filter<'_> {
    fn excluded(&self, r: &RectPt) -> bool {
        let start = self.sorted.partition_point(|e| e.x0 < r.x0 - MATCH_TOL);
        self.sorted[start..]
            .iter()
            .take_while(|e| e.x0 <= r.x0 + MATCH_TOL)
            .any(|e| e.approx_eq(r, MATCH_TOL))
    }
}

impl<'a> RenderFilter<'a> for Filter<'_> {
    fn keep_glyph_run(&mut self, run: &GlyphRun<'_, 'a>, props: &DrawProps<'a>, _: &DrawMode) -> bool {
        if !self.ex.text {
            return true;
        }
        let Some(first) = run.glyphs().first() else {
            return true;
        };
        let t = self.to_page * props.transform * first.transform();
        if !is_horizontal(&t) {
            // Rotated text isn't in the text layer; the raster layer draws it.
            return true;
        }
        let [.., e, f] = t.as_coeffs();
        let p = PointPt {
            x: e as f32,
            y: f as f32,
        };
        self.ex.keep_text.iter().any(|r| r.contains(p))
    }

    fn filter_path<'p>(&mut self, path: &'p BezPath, props: &DrawProps<'a>, _: &DrawMode) -> Option<Cow<'p, BezPath>> {
        if self.sorted.is_empty() {
            return Some(Cow::Borrowed(path));
        }
        let t = self.to_page * props.transform;
        let parts = split_subpaths(path);
        let mut keep = BezPath::new();
        let mut dropped = false;
        for els in parts {
            if self.excluded(&subpath_in(els, t).bbox) {
                dropped = true;
            } else {
                for el in els {
                    keep.push(*el);
                }
            }
        }
        if !dropped {
            Some(Cow::Borrowed(path))
        } else if keep.elements().is_empty() {
            None
        } else {
            Some(Cow::Owned(keep))
        }
    }
}

pub(super) fn render(
    doc: &HayroDoc,
    page: &Page<'_>,
    region: RectPt,
    px_w: u32,
    px_h: u32,
    ex: &Exclusions,
) -> RgbaImage {
    let w = px_w.clamp(1, u16::MAX as u32) as u16;
    let h = px_h.clamp(1, u16::MAX as u32) as u16;
    let sx = w as f64 / region.width().max(1e-3) as f64;
    let sy = h as f64 / region.height().max(1e-3) as f64;
    let view = Affine::scale_non_uniform(sx, sy) * Affine::translate((-region.x0 as f64, -region.y0 as f64));
    let transform = view * HayroDoc::page_transform(page);
    let mut sorted = ex.paths.clone();
    sorted.sort_by(|a, b| a.x0.total_cmp(&b.x0));
    let mut filter = Filter {
        ex,
        to_page: view.inverse(),
        sorted,
    };
    let cache = RenderCache::new();
    let mut ctx = RenderContext::new(w, h);
    render_into_filtered(
        page,
        &cache,
        &doc.settings,
        &RenderSettings::default(),
        &mut ctx,
        transform,
        &mut filter,
    );
    ctx.flush();
    let mut pixmap = Pixmap::new(w, h);
    ctx.render_with(
        &mut pixmap,
        &mut vello_cpu::Resources::default(),
        vello_cpu::RasterizerSettings {
            target_init: vello_cpu::TargetInit::Clear(vello_cpu::color::palette::css::WHITE),
            ..Default::default()
        },
    );
    // Rendered over opaque white, so premultiplied equals straight alpha.
    RgbaImage {
        width: w as u32,
        height: h as u32,
        data: pixmap.data_as_u8_slice().to_vec(),
    }
}
