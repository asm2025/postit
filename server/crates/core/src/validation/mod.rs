//! Post validation: machine-readable violations and the engine that produces them.

mod text;
mod violation;

pub use text::{TextMeasure, measure};
pub use violation::{AspectRatio, AspectRatioError, SuggestedFix, Violation, ViolationCode};
