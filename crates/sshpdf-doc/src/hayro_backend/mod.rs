//! The hayro backend (pure Rust, Apache-2.0 / MIT).
//!
//! Uses a patched copy of the `hayro` renderer (see `vendor/README.md`) for
//! rendering with exclusions; text, paths, outline and links come from
//! `hayro-interpret` and `hayro-syntax` directly.

mod graphics;
mod nav;
mod render;
mod text;

use std::collections::HashMap;
use std::sync::Arc;

use hayro::hayro_interpret::InterpreterSettings;
use hayro::hayro_interpret::util::TransformExt;
use hayro::hayro_syntax::object::ObjectIdentifier;
use hayro::hayro_syntax::page::Page;
use hayro::hayro_syntax::{DecryptionError, LoadPdfError, Pdf};
use hayro::kurbo::Affine;

use crate::{
    Backend, Doc, Exclusions, Link, OpenError, OutlineItem, PageGraphics, PageText, RectPt, RgbaImage, SizePt,
};

/// The hayro backend.
pub struct HayroBackend;

impl Backend for HayroBackend {
    fn name(&self) -> &'static str {
        "hayro"
    }

    fn open(&self, data: Vec<u8>, password: Option<&str>) -> Result<Box<dyn Doc>, OpenError> {
        let pdf = Pdf::new_with_password(data, password.unwrap_or("")).map_err(|e| match e {
            LoadPdfError::Decryption(DecryptionError::PasswordProtected) => {
                if password.is_some_and(|p| !p.is_empty()) {
                    OpenError::WrongPassword
                } else {
                    OpenError::PasswordRequired
                }
            }
            LoadPdfError::Decryption(DecryptionError::UnsupportedAlgorithm) => OpenError::UnsupportedEncryption,
            LoadPdfError::Decryption(e) => OpenError::Invalid(format!("{e:?}")),
            LoadPdfError::Invalid => OpenError::Invalid("not a PDF or damaged beyond repair".into()),
        })?;
        Ok(Box::new(HayroDoc::new(pdf)))
    }
}

pub(crate) struct HayroDoc {
    pdf: Pdf,
    settings: InterpreterSettings,
    /// Object id of each page dictionary → page index.
    page_ids: HashMap<ObjectIdentifier, usize>,
}

impl HayroDoc {
    fn new(pdf: Pdf) -> HayroDoc {
        let mut page_ids = HashMap::new();
        for (i, page) in pdf.pages().iter().enumerate() {
            if let Some(id) = page.raw().obj_id() {
                page_ids.insert(id, i);
            }
        }
        let settings = InterpreterSettings {
            warning_sink: Arc::new(|w| log::debug!("hayro: {w:?}")),
            ..Default::default()
        };
        HayroDoc {
            pdf,
            settings,
            page_ids,
        }
    }

    fn page(&self, i: usize) -> Option<&Page<'_>> {
        self.pdf.pages().get(i)
    }

    /// PDF user space → page space (y down, rotation applied, crop box
    /// origin at 0,0).
    fn page_transform(page: &Page<'_>) -> Affine {
        page.initial_transform(true).to_kurbo()
    }
}

impl Doc for HayroDoc {
    fn page_count(&self) -> usize {
        self.pdf.pages().len()
    }

    fn page_size(&self, page: usize) -> SizePt {
        match self.page(page) {
            Some(p) => {
                let (w, h) = p.render_dimensions();
                SizePt { w, h }
            }
            None => SizePt { w: 612.0, h: 792.0 },
        }
    }

    fn outline(&self) -> Vec<OutlineItem> {
        nav::outline(self)
    }

    fn text(&self, page: usize) -> PageText {
        match self.page(page) {
            Some(p) => text::extract(self, p),
            None => PageText::default(),
        }
    }

    fn graphics(&self, page: usize) -> PageGraphics {
        match self.page(page) {
            Some(p) => graphics::extract(self, p),
            None => PageGraphics::default(),
        }
    }

    fn render(&self, page: usize, region: RectPt, px_w: u32, px_h: u32, exclude: &Exclusions) -> RgbaImage {
        match self.page(page) {
            Some(p) => render::render(self, p, region, px_w, px_h, exclude),
            None => RgbaImage {
                width: px_w,
                height: px_h,
                data: vec![255; px_w as usize * px_h as usize * 4],
            },
        }
    }

    fn links(&self, page: usize) -> Vec<Link> {
        match self.page(page) {
            Some(p) => nav::links(self, p, page),
            None => Vec::new(),
        }
    }

    fn title(&self) -> Option<String> {
        let t = self.pdf.metadata().title.as_ref()?;
        let s = nav::decode_text_string(t);
        (!s.trim().is_empty()).then_some(s)
    }
}
