//! The terminal side of sshpdf: capability detection, the cell grid, and
//! turning cells into escape sequences.

pub mod backend;
pub mod caps;
pub mod cell;
pub mod palette;
pub mod sgr;

pub use caps::{Caps, ColorDepth, Tier};
pub use cell::{Attrs, Cell, CellGrid, Color, Wide, char_width};
