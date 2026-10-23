//! Map rendering: layers projected and simplified into levels of detail,
//! drawn with egui.

pub mod mesh;
pub mod paint;
pub mod view;

pub use mesh::{Bounds, LayerMesh, Level};
pub use view::View;
