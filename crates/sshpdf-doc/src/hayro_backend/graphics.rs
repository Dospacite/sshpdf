//! Vector paths and image positions, for the line layer and the theme.

use hayro::hayro_interpret::font::GlyphRun;
use hayro::hayro_interpret::{
    BlendMode, ClipPath, Context, Device, DrawMode, DrawProps, Image, ImageDrawProps, InterpreterCache, Paint,
    SoftMask, StrokeProps, interpret_page,
};
use hayro::hayro_syntax::page::Page;
use hayro::kurbo::{Affine, BezPath, PathEl, Point, Rect, Shape};

use super::HayroDoc;
use crate::{PageGraphics, PathItem, PointPt, RectPt, Rgb, StrokeStyle, SubPath};

/// Split a path into subpaths (in the path's own coordinates).
pub(super) fn split_subpaths(path: &BezPath) -> Vec<&[PathEl]> {
    let els = path.elements();
    let mut out = Vec::new();
    let mut start = 0;
    for (i, el) in els.iter().enumerate() {
        if matches!(el, PathEl::MoveTo(_)) && i > start {
            out.push(&els[start..i]);
            start = i;
        }
    }
    if start < els.len() {
        out.push(&els[start..]);
    }
    out
}

/// A subpath mapped by `t`: its vertices (curve end points included) and
/// whether it has curves and is closed. Shared by extraction and the render
/// filter so both compute identical bounding boxes.
pub(super) fn subpath_in(els: &[PathEl], t: Affine) -> SubPath {
    let mut points = Vec::with_capacity(els.len());
    let mut curved = false;
    let mut closed = false;
    let mut push = |p: Point| {
        let q = t * p;
        points.push(PointPt {
            x: q.x as f32,
            y: q.y as f32,
        });
    };
    for el in els {
        match *el {
            PathEl::MoveTo(p) | PathEl::LineTo(p) => push(p),
            PathEl::QuadTo(_, p) | PathEl::CurveTo(_, _, p) => {
                curved = true;
                push(p);
            }
            PathEl::ClosePath => closed = true,
        }
    }
    let bbox = bbox_of(&points);
    SubPath {
        points,
        closed,
        curved,
        bbox,
    }
}

pub(super) fn bbox_of(points: &[PointPt]) -> RectPt {
    let mut r = RectPt {
        x0: f32::MAX,
        y0: f32::MAX,
        x1: f32::MIN,
        y1: f32::MIN,
    };
    for p in points {
        r.x0 = r.x0.min(p.x);
        r.y0 = r.y0.min(p.y);
        r.x1 = r.x1.max(p.x);
        r.y1 = r.y1.max(p.y);
    }
    if points.is_empty() { RectPt::default() } else { r }
}

/// Average scale factor of a transform (for line widths).
pub(super) fn scale_of(t: &Affine) -> f64 {
    let [a, b, c, d, _, _] = t.as_coeffs();
    (a * d - b * c).abs().sqrt()
}

fn rgb_of(paint: &Paint<'_>) -> Rgb {
    match paint {
        Paint::Color(c) => {
            let [r, g, b, _] = c.to_rgba().to_rgba8();
            [r, g, b]
        }
        Paint::Pattern(_) => [128, 128, 128],
    }
}

#[derive(Default)]
struct GraphicsDevice {
    out: PageGraphics,
}

impl GraphicsDevice {
    fn add(&mut self, path: &BezPath, props: &DrawProps<'_>, mode: &DrawMode) {
        let color = rgb_of(&props.paint);
        let stroke_of = |s: &StrokeProps| StrokeStyle {
            width: (s.line_width as f64 * scale_of(&props.transform)) as f32,
            color,
            dashed: !s.dash_array.is_empty(),
        };
        let (fill, stroke) = match mode {
            DrawMode::Fill(_) => (Some(color), None),
            DrawMode::Stroke(s) => (None, Some(stroke_of(s))),
            DrawMode::FillAndStroke(_, s) => (Some(color), Some(stroke_of(s))),
            DrawMode::Invisible => return,
        };
        let subpaths = split_subpaths(path)
            .into_iter()
            .map(|els| subpath_in(els, props.transform))
            .filter(|s| !s.points.is_empty())
            .collect::<Vec<_>>();
        if !subpaths.is_empty() {
            self.out.paths.push(PathItem { subpaths, stroke, fill });
        }
    }
}

impl<'a> Device<'a> for GraphicsDevice {
    fn draw_path(&mut self, path: &BezPath, props: DrawProps<'a>, mode: &DrawMode) {
        self.add(path, &props, mode);
    }

    fn draw_rect(&mut self, rect: &Rect, props: DrawProps<'a>, mode: &DrawMode) {
        self.add(&rect.to_path(0.1), &props, mode);
    }

    fn push_clip_path(&mut self, _: &ClipPath) {}
    fn push_transparency_group(&mut self, _: f32, _: Option<SoftMask<'a>>, _: BlendMode) {}
    fn draw_glyph_run(&mut self, _: &GlyphRun<'_, 'a>, _: DrawProps<'a>, _: &DrawMode) {}

    fn draw_image(&mut self, image: Image<'a, '_>, props: ImageDrawProps<'a>) {
        let local = Rect::new(0.0, 0.0, image.width() as f64, image.height() as f64);
        let r = props.transform.transform_rect_bbox(local);
        self.out
            .images
            .push(RectPt::new(r.x0 as f32, r.y0 as f32, r.x1 as f32, r.y1 as f32));
    }

    fn pop_clip(&mut self) {}
    fn pop_transparency_group(&mut self) {}
}

pub(super) fn extract(doc: &HayroDoc, page: &Page<'_>) -> PageGraphics {
    let (w, h) = page.render_dimensions();
    let cache = InterpreterCache::new();
    let mut ctx = Context::new(
        HayroDoc::page_transform(page),
        Rect::new(0.0, 0.0, w as f64, h as f64),
        &cache,
        page.xref(),
        doc.settings.clone(),
    );
    let mut dev = GraphicsDevice::default();
    interpret_page(page, &mut ctx, &mut dev);
    dev.out
}
