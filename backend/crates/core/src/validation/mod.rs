//! Post validation: machine-readable violations and the engine that produces them.

mod engine;
mod format_pick;
mod schedule;
mod text;
mod violation;

pub use engine::{Schedule, ValidationInput, validate_target};
pub use format_pick::pick_format;
pub use schedule::{ScheduleInput, parse_zone, resolve_schedule};
pub use text::{TextMeasure, measure};
pub use violation::{AspectRatio, AspectRatioError, SuggestedFix, Violation, ViolationCode};
