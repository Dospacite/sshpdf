//! The raster layer of sshpdf: glyph sets, the per-cell optimizer, color
//! quantization and the mapping from page colors to terminal colors.

pub mod color;
pub mod glyphs;
pub mod optimize;
pub mod theme;

pub use glyphs::{GlyphSet, N, SH, SW};
pub use optimize::{LabImage, Options, RasterCell, optimize, optimize_lab};
pub use theme::{Ink, Theme};
