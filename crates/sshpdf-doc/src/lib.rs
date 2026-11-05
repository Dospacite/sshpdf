//! PDF access for sshpdf: the backend trait and its hayro implementation.
//!
//! Everything is in **page space**: PDF points, origin at the top-left of the
//! visible page (crop box), y pointing down, page rotation already applied.

mod fontstyle;
pub mod group;
mod hayro_backend;

use std::fmt;
use std::path::Path;

pub use group::RawGlyph;
pub use hayro_backend::HayroBackend;

/// An sRGB color.
pub type Rgb = [u8; 3];

#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct PointPt {
    pub x: f32,
    pub y: f32,
}

/// A rectangle in page space; `x0 <= x1`, `y0 <= y1`.
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct RectPt {
    pub x0: f32,
    pub y0: f32,
    pub x1: f32,
    pub y1: f32,
}

impl RectPt {
    pub fn new(x0: f32, y0: f32, x1: f32, y1: f32) -> RectPt {
        RectPt {
            x0: x0.min(x1),
            y0: y0.min(y1),
            x1: x0.max(x1),
            y1: y0.max(y1),
        }
    }

    pub fn width(&self) -> f32 {
        self.x1 - self.x0
    }

    pub fn height(&self) -> f32 {
        self.y1 - self.y0
    }

    pub fn union(&self, o: &RectPt) -> RectPt {
        RectPt {
            x0: self.x0.min(o.x0),
            y0: self.y0.min(o.y0),
            x1: self.x1.max(o.x1),
            y1: self.y1.max(o.y1),
        }
    }

    pub fn intersects(&self, o: &RectPt) -> bool {
        self.x0 < o.x1 && o.x0 < self.x1 && self.y0 < o.y1 && o.y0 < self.y1
    }

    pub fn contains(&self, p: PointPt) -> bool {
        p.x >= self.x0 && p.x <= self.x1 && p.y >= self.y0 && p.y <= self.y1
    }

    pub fn center(&self) -> PointPt {
        PointPt {
            x: (self.x0 + self.x1) / 2.0,
            y: (self.y0 + self.y1) / 2.0,
        }
    }

    /// Whether two rectangles are equal within `tol` on every edge.
    pub fn approx_eq(&self, o: &RectPt, tol: f32) -> bool {
        (self.x0 - o.x0).abs() <= tol
            && (self.y0 - o.y0).abs() <= tol
            && (self.x1 - o.x1).abs() <= tol
            && (self.y1 - o.y1).abs() <= tol
    }
}

#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct SizePt {
    pub w: f32,
    pub h: f32,
}

/// A font used on a page.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct FontInfo {
    /// PostScript name without the subset prefix (`ABCDEF+`).
    pub name: String,
    pub bold: bool,
    pub italic: bool,
    pub mono: bool,
}

/// Flags of a [`TextChar`].
pub mod char_flags {
    pub const BOLD: u8 = 1;
    pub const ITALIC: u8 = 2;
    pub const MONO: u8 = 4;
    /// Text render mode 3 (e.g. an OCR layer over a scan).
    pub const INVISIBLE: u8 = 8;
    pub const SUPERSCRIPT: u8 = 16;
    pub const SUBSCRIPT: u8 = 32;
}

/// One character of the text layer.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct TextChar {
    pub c: char,
    /// From the glyph origin to its advance, and from about the ascender to
    /// the descender.
    pub bbox: RectPt,
    /// Baseline (page-space y).
    pub baseline: f32,
    /// Font size in points.
    pub size: f32,
    pub color: Rgb,
    /// Index into [`PageText::fonts`].
    pub font: u16,
    /// See [`char_flags`].
    pub flags: u8,
}

impl TextChar {
    pub fn has(&self, flag: u8) -> bool {
        self.flags & flag != 0
    }
}

#[derive(Clone, Debug, Default, PartialEq)]
pub struct TextWord {
    pub chars: Vec<TextChar>,
    pub bbox: RectPt,
}

impl TextWord {
    pub fn text(&self) -> String {
        self.chars.iter().map(|c| c.c).collect()
    }
}

/// A line of horizontal text: words on one baseline, left to right.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct TextLine {
    pub words: Vec<TextWord>,
    pub bbox: RectPt,
    pub baseline: f32,
    /// Main font size of the line (scripts excluded).
    pub size: f32,
}

impl TextLine {
    pub fn text(&self) -> String {
        let words: Vec<String> = self.words.iter().map(TextWord::text).collect();
        words.join(" ")
    }

    pub fn char_count(&self) -> usize {
        self.words.iter().map(|w| w.chars.len()).sum()
    }
}

/// A block of lines (about a paragraph).
#[derive(Clone, Debug, Default, PartialEq)]
pub struct TextBlock {
    pub lines: Vec<TextLine>,
    pub bbox: RectPt,
}

/// The text layer of a page.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct PageText {
    /// Blocks in reading order.
    pub blocks: Vec<TextBlock>,
    pub fonts: Vec<FontInfo>,
    /// Text that isn't horizontal and upright (rotated labels, vertical
    /// text). Left to the raster layer.
    pub other: Vec<TextChar>,
    /// Glyphs without a Unicode mapping (shown as `?`).
    pub unmapped: usize,
}

impl PageText {
    pub fn lines(&self) -> impl Iterator<Item = &TextLine> {
        self.blocks.iter().flat_map(|b| b.lines.iter())
    }

    pub fn is_empty(&self) -> bool {
        self.blocks.is_empty()
    }

    pub fn char_count(&self) -> usize {
        self.lines().map(TextLine::char_count).sum()
    }
}

/// How a path is stroked.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct StrokeStyle {
    /// Line width in points.
    pub width: f32,
    pub color: Rgb,
    pub dashed: bool,
}

/// One subpath, in page space.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct SubPath {
    /// The vertices of the straight segments (for curved subpaths, the
    /// end points of all segments).
    pub points: Vec<PointPt>,
    pub closed: bool,
    /// Has curve segments.
    pub curved: bool,
    pub bbox: RectPt,
}

/// A painted path.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct PathItem {
    pub subpaths: Vec<SubPath>,
    pub stroke: Option<StrokeStyle>,
    pub fill: Option<Rgb>,
}

/// The vector graphics and image positions of a page.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct PageGraphics {
    pub paths: Vec<PathItem>,
    /// Bounding boxes of drawn images.
    pub images: Vec<RectPt>,
}

/// A place in the document.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Dest {
    pub page: usize,
    /// Page-space y of the target, if given.
    pub y: Option<f32>,
}

#[derive(Clone, Debug, PartialEq)]
pub struct OutlineItem {
    pub title: String,
    pub dest: Option<Dest>,
    pub children: Vec<OutlineItem>,
}

#[derive(Clone, Debug, PartialEq)]
pub enum LinkTarget {
    Internal(Dest),
    External(String),
}

#[derive(Clone, Debug, PartialEq)]
pub struct Link {
    pub rect: RectPt,
    pub target: LinkTarget,
}

/// What [`Doc::render`] leaves out, so the raster layer doesn't draw
/// content again that the text and line layers already show.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct Exclusions {
    /// Leave out horizontal, upright text...
    pub text: bool,
    /// ...except glyph runs whose first glyph origin lies in one of these
    /// rectangles (text the text layer couldn't place).
    pub keep_text: Vec<RectPt>,
    /// Leave out subpaths whose page-space bounding box matches one of
    /// these (the snapped lines).
    pub paths: Vec<RectPt>,
}

impl Exclusions {
    pub fn none() -> Exclusions {
        Exclusions::default()
    }
}

/// An RGBA8 image (not premultiplied).
#[derive(Clone, Debug, PartialEq)]
pub struct RgbaImage {
    pub width: u32,
    pub height: u32,
    pub data: Vec<u8>,
}

/// Why a document could not be opened.
#[derive(Debug, Clone, PartialEq)]
pub enum OpenError {
    /// The document is encrypted and needs a password.
    PasswordRequired,
    /// The given password is wrong.
    WrongPassword,
    /// The encryption method is not supported.
    UnsupportedEncryption,
    /// Not a PDF, or too broken to read.
    Invalid(String),
}

impl fmt::Display for OpenError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            OpenError::PasswordRequired => f.write_str("the document is encrypted and needs a password"),
            OpenError::WrongPassword => f.write_str("wrong password"),
            OpenError::UnsupportedEncryption => f.write_str("unsupported encryption"),
            OpenError::Invalid(why) => write!(f, "cannot read PDF: {why}"),
        }
    }
}

impl std::error::Error for OpenError {}

/// An open document.
pub trait Doc: Send + Sync {
    fn page_count(&self) -> usize;
    /// Size of the page as displayed (rotation applied), in points.
    fn page_size(&self, page: usize) -> SizePt;
    fn outline(&self) -> Vec<OutlineItem>;
    /// The text layer of a page.
    fn text(&self, page: usize) -> PageText;
    /// Vector paths and image positions of a page.
    fn graphics(&self, page: usize) -> PageGraphics;
    /// Render `region` (page space) of a page to `px_w` x `px_h` pixels over
    /// white, leaving out what `exclude` says.
    fn render(&self, page: usize, region: RectPt, px_w: u32, px_h: u32, exclude: &Exclusions) -> RgbaImage;
    fn links(&self, page: usize) -> Vec<Link>;
    /// Document title from the metadata, if any.
    fn title(&self) -> Option<String> {
        None
    }
}

/// A PDF library that can open documents.
pub trait Backend {
    fn name(&self) -> &'static str;
    fn open(&self, data: Vec<u8>, password: Option<&str>) -> Result<Box<dyn Doc>, OpenError>;
}

/// Open a file with the default backend.
pub fn open_file(path: &Path, password: Option<&str>) -> Result<Box<dyn Doc>, OpenError> {
    let data = std::fs::read(path).map_err(|e| OpenError::Invalid(e.to_string()))?;
    HayroBackend.open(data, password)
}
