//! An open document with per-page caches of its text and graphics.

use std::path::{Path, PathBuf};
use std::sync::{Arc, OnceLock};

use sshpdf_doc::{Doc, Exclusions, OpenError, PageGraphics, PageText, RectPt, RgbaImage, SizePt};
use sshpdf_layout::{Mapping, PageLayout};

use crate::config::{ViewMode, ZoomSpec};

/// An open document. Cheap to share between threads (`Arc<Session>`).
pub struct Session {
    pub path: PathBuf,
    pub doc: Arc<dyn Doc>,
    text: Vec<OnceLock<Arc<PageText>>>,
    graphics: Vec<OnceLock<Arc<PageGraphics>>>,
    photos: Vec<OnceLock<Arc<Vec<RectPt>>>>,
}

impl Session {
    /// Open a file. An empty password is always tried first, so files
    /// encrypted with only an owner password open without asking.
    pub fn open(path: &Path, password: Option<&str>) -> Result<Session, OpenError> {
        let data = std::fs::read(path).map_err(|e| OpenError::Invalid(format!("{}: {e}", path.display())))?;
        let backend = sshpdf_doc::HayroBackend;
        use sshpdf_doc::Backend;
        let doc = match backend.open(data.clone(), None) {
            Ok(d) => d,
            Err(OpenError::PasswordRequired) => match password {
                Some(p) => backend.open(data, Some(p))?,
                None => return Err(OpenError::PasswordRequired),
            },
            Err(e) => return Err(e),
        };
        Ok(Session::from_doc(path.to_path_buf(), Arc::from(doc)))
    }

    pub fn from_doc(path: PathBuf, doc: Arc<dyn Doc>) -> Session {
        let n = doc.page_count();
        Session {
            path,
            doc,
            text: (0..n).map(|_| OnceLock::new()).collect(),
            graphics: (0..n).map(|_| OnceLock::new()).collect(),
            photos: (0..n).map(|_| OnceLock::new()).collect(),
        }
    }

    pub fn page_count(&self) -> usize {
        self.text.len()
    }

    pub fn page_size(&self, page: usize) -> SizePt {
        self.doc.page_size(page)
    }

    pub fn text(&self, page: usize) -> Arc<PageText> {
        match self.text.get(page) {
            Some(slot) => slot.get_or_init(|| Arc::new(self.doc.text(page))).clone(),
            None => Arc::new(PageText::default()),
        }
    }

    pub fn graphics(&self, page: usize) -> Arc<PageGraphics> {
        match self.graphics.get(page) {
            Some(slot) => slot.get_or_init(|| Arc::new(self.doc.graphics(page))).clone(),
            None => Arc::new(PageGraphics::default()),
        }
    }

    /// The text zoom around a page: the median line spacing and stretch of
    /// up to five nearby pages with text (one title page doesn't decide),
    /// rows aligned to this page's text. Fit width if there is no text.
    pub fn text_zoom(&self, page: usize, cols: u16, aspect: f32) -> Mapping {
        let n = self.page_count();
        let mut pitches = Vec::new();
        let mut stretches = Vec::new();
        'outer: for d in 0..n.min(16) {
            for p in [page + d, page.wrapping_sub(d)] {
                if p < n
                    && (d > 0 || p == page)
                    && let Some(m) = Mapping::text_zoom(&self.text(p), aspect)
                {
                    pitches.push(m.row_pitch);
                    stretches.push(m.x_stretch);
                    if pitches.len() >= 5 {
                        break 'outer;
                    }
                }
                if d == 0 {
                    break;
                }
            }
        }
        if pitches.is_empty() {
            return Mapping::fit_width(self.page_size(page), cols, aspect);
        }
        let median = |v: &mut Vec<f32>| {
            v.sort_by(f32::total_cmp);
            v[v.len() / 2]
        };
        let m = Mapping::new(median(&mut pitches), aspect).with_stretch(median(&mut stretches));
        self.with_phase(page, m)
    }

    /// Align the rows of `m` to the text of `page` (see `Mapping::y_offset`).
    pub fn with_phase(&self, page: usize, m: Mapping) -> Mapping {
        Mapping {
            y_offset: sshpdf_layout::mapping::text_phase(&self.text(page), &m),
            ..m
        }
    }

    /// The mapping for a zoom spec on a `cols` x `rows` viewport. Raster
    /// mode keeps the true aspect (no horizontal stretch).
    pub fn mapping(&self, zoom: ZoomSpec, mode: ViewMode, page: usize, cols: u16, rows: u16, aspect: f32) -> Mapping {
        let size = self.page_size(page);
        let m = match zoom {
            ZoomSpec::Text => self.text_zoom(page, cols, aspect),
            ZoomSpec::Width => Mapping::fit_width(size, cols, aspect),
            ZoomSpec::Page => Mapping::fit_page(size, cols, rows, aspect),
            ZoomSpec::Percent(p) => Mapping::from_zoom_percent(p, aspect),
        };
        if mode == ViewMode::Raster {
            m.with_stretch(1.0)
        } else {
            m
        }
    }

    /// Build the layout of a page for a view mode (reflow pages have none).
    pub fn layout(&self, page: usize, mapping: Mapping, mode: ViewMode) -> PageLayout {
        let size = self.page_size(page);
        let mut layout = match mode {
            ViewMode::Raster => PageLayout::raster(page, size, &self.graphics(page), mapping),
            _ => PageLayout::layout(page, size, &self.text(page), &self.graphics(page), mapping),
        };
        layout.photos = self.photos(page).to_vec();
        layout
    }

    /// The images of a page that are photos (shown in real colors). Images
    /// that are mostly one flat color (slide backgrounds, charts and
    /// screenshots on white) follow the terminal theme like vector graphics.
    pub fn photos(&self, page: usize) -> Arc<Vec<RectPt>> {
        let Some(slot) = self.photos.get(page) else {
            return Arc::new(Vec::new());
        };
        slot.get_or_init(|| {
            let images = &self.graphics(page).images;
            if images.is_empty() {
                return Arc::new(Vec::new());
            }
            let size = self.page_size(page);
            // About 2 pixels per point is plenty to tell a photo from a fill.
            let scale = (400.0 / size.w.max(size.h)).min(2.0);
            let (w, h) = ((size.w * scale).ceil() as u32, (size.h * scale).ceil() as u32);
            let page_rect = RectPt::new(0.0, 0.0, size.w, size.h);
            let img = self.doc.render(page, page_rect, w, h, &Exclusions::none());
            let photos = images.iter().filter(|r| is_photo(&img, r, scale)).copied().collect();
            Arc::new(photos)
        })
        .clone()
    }

    /// A page with no text layer that is mostly one image: a scan.
    pub fn is_scanned(&self, page: usize) -> bool {
        let text = self.text(page);
        if text.char_count() > 0 {
            return false;
        }
        let size = self.page_size(page);
        let area = size.w * size.h;
        self.graphics(page)
            .images
            .iter()
            .any(|r| r.width() * r.height() > 0.5 * area)
    }

    /// File name for display.
    pub fn file_name(&self) -> String {
        self.path
            .file_name()
            .map(|n| n.to_string_lossy().into_owned())
            .unwrap_or_else(|| self.path.display().to_string())
    }
}

/// Whether the pixels of `r` (page space, `scale` pixels per point) look
/// like a photo: no single color covers most of it.
fn is_photo(img: &RgbaImage, r: &RectPt, scale: f32) -> bool {
    let x0 = ((r.x0 * scale).floor().max(0.0) as u32).min(img.width);
    let x1 = ((r.x1 * scale).ceil().max(0.0) as u32).min(img.width);
    let y0 = ((r.y0 * scale).floor().max(0.0) as u32).min(img.height);
    let y1 = ((r.y1 * scale).ceil().max(0.0) as u32).min(img.height);
    if x1 <= x0 || y1 <= y0 {
        return false;
    }
    // Count colors on a coarse grid (16 levels per channel).
    let mut counts = std::collections::HashMap::<u16, u32>::new();
    let mut total = 0u32;
    for y in y0..y1 {
        for x in x0..x1 {
            let i = ((y * img.width + x) * 4) as usize;
            let q = |v: u8| (v >> 4) as u16;
            let key = q(img.data[i]) << 8 | q(img.data[i + 1]) << 4 | q(img.data[i + 2]);
            *counts.entry(key).or_insert(0) += 1;
            total += 1;
        }
    }
    let top = counts.values().copied().max().unwrap_or(0);
    // A photo has no dominant color.
    (top as f32) < 0.5 * total as f32
}

#[cfg(test)]
mod tests {
    use super::*;

    fn image(w: u32, h: u32, f: impl Fn(u32, u32) -> [u8; 3]) -> RgbaImage {
        let mut data = Vec::new();
        for y in 0..h {
            for x in 0..w {
                let [r, g, b] = f(x, y);
                data.extend_from_slice(&[r, g, b, 255]);
            }
        }
        RgbaImage {
            width: w,
            height: h,
            data,
        }
    }

    #[test]
    fn photos_versus_flat_images() {
        let flat = image(20, 20, |x, _| if x < 3 { [0, 0, 0] } else { [255, 255, 255] });
        assert!(!is_photo(&flat, &RectPt::new(0.0, 0.0, 20.0, 20.0), 1.0));
        let photo = image(20, 20, |x, y| [(x * 12) as u8, (y * 12) as u8, ((x + y) * 6) as u8]);
        assert!(is_photo(&photo, &RectPt::new(0.0, 0.0, 20.0, 20.0), 1.0));
        assert!(!is_photo(&photo, &RectPt::new(30.0, 30.0, 40.0, 40.0), 1.0));
    }
}
