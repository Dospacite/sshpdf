//! Layout for sshpdf: the page-to-cell mapping, the text layer, the line
//! layer (box drawing), composition of all layers, and reflow.

pub mod boxchars;
pub mod compose;
pub mod lines;
pub mod mapping;
pub mod reflow;
pub mod search;
pub mod textlayer;

pub use compose::{Mode, PageLayout, RenderOptions, render_cells};
pub use mapping::{CellRect, Mapping};
