//! Thematic mapping: colours, ramps, classification and legends.

pub mod bivariate;
pub mod classify;
pub mod color;
pub mod format;
pub mod ramps;
pub mod thematic;

pub use bivariate::{BIVARIATE, BivariateGrid, BivariateScale};
pub use classify::Method;
pub use color::Rgba;
pub use ramps::{RAMPS, Ramp, RampKind, RampSpec};
pub use thematic::{CategoryScale, ColorScale, LegendEntry, ScaleMode, Scaled};
