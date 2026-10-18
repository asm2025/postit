//! Post validation: machine-readable violations and the engine that produces them.

mod violation;

pub use violation::{AspectRatio, AspectRatioError, SuggestedFix, Violation, ViolationCode};
