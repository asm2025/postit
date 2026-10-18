//! Recurring posts: the small subset of iCalendar `RRULE` the product offers.
//!
//! Only the canonical spelling is accepted, so a stored rule always compares and reads the same
//! way: uppercase `KEY=VALUE` pairs joined by `;`, no `RRULE:` prefix, no empty segments.

use std::collections::BTreeSet;
use std::fmt;
use std::num::NonZeroU32;
use std::str::FromStr;

use chrono::{DateTime, NaiveDateTime, Utc};
use serde::{Deserialize, Deserializer, Serialize, Serializer};

const MAX_INTERVAL: u16 = 365;
const MAX_COUNT: u32 = 1000;
const UNTIL_FORMAT: &str = "%Y%m%dT%H%M%SZ";
const UNTIL_LEN: usize = 16;

/// A day of the week, Monday first.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum Weekday {
    /// Monday.
    Mo,
    /// Tuesday.
    Tu,
    /// Wednesday.
    We,
    /// Thursday.
    Th,
    /// Friday.
    Fr,
    /// Saturday.
    Sa,
    /// Sunday.
    Su,
}

impl Weekday {
    const fn as_str(self) -> &'static str {
        match self {
            Self::Mo => "MO",
            Self::Tu => "TU",
            Self::We => "WE",
            Self::Th => "TH",
            Self::Fr => "FR",
            Self::Sa => "SA",
            Self::Su => "SU",
        }
    }

    fn parse(value: &str) -> Option<Self> {
        Some(match value {
            "MO" => Self::Mo,
            "TU" => Self::Tu,
            "WE" => Self::We,
            "TH" => Self::Th,
            "FR" => Self::Fr,
            "SA" => Self::Sa,
            "SU" => Self::Su,
            _ => return None,
        })
    }
}

/// How often a rule repeats.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Frequency {
    /// Every `interval` days.
    Daily,
    /// Every `interval` weeks, on these days.
    Weekly {
        /// The days of the week; never empty.
        days: BTreeSet<Weekday>,
    },
    /// Every `interval` months, on this day of the month.
    Monthly {
        /// Day of the month, 1 to 31. Months without that day are skipped by the scheduler.
        day: u8,
    },
}

impl Frequency {
    const fn name(&self) -> &'static str {
        match self {
            Self::Daily => "DAILY",
            Self::Weekly { .. } => "WEEKLY",
            Self::Monthly { .. } => "MONTHLY",
        }
    }
}

/// When a rule stops.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum RecurrenceEnd {
    /// Repeats until the user stops it.
    Never,
    /// Stops after this many occurrences.
    Count(NonZeroU32),
    /// Stops after this moment.
    Until(DateTime<Utc>),
}

/// A recurrence rule. Parses from and prints as its canonical `RRULE` string, which is also its
/// wire form.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RecurrenceRule {
    /// What repeats.
    pub frequency: Frequency,
    /// Repeat every this many days, weeks, or months; 1 to 365.
    pub interval: u16,
    /// When the repetition stops.
    pub end: RecurrenceEnd,
}

/// Why a string is not an accepted rule.
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
pub enum RecurrenceError {
    /// The string is empty.
    #[error("the rule is empty")]
    Empty,
    /// A segment is empty or is not `KEY=VALUE`.
    #[error("malformed segment {0:?}")]
    Malformed(String),
    /// A key appears twice.
    #[error("{0} appears more than once")]
    DuplicateKey(String),
    /// The key is not part of the accepted subset (or not in canonical uppercase).
    #[error("{0} is not supported")]
    UnsupportedKey(String),
    /// `FREQ` is not `DAILY`, `WEEKLY`, or `MONTHLY`.
    #[error("frequency {0} is not supported")]
    UnsupportedFrequency(String),
    /// A required key is absent.
    #[error("{0} is required")]
    MissingKey(&'static str),
    /// A key has a value outside its accepted range or format.
    #[error("{value:?} is not a valid {key}")]
    InvalidValue {
        /// The key.
        key: &'static str,
        /// The offending value.
        value: String,
    },
    /// `COUNT` and `UNTIL` are both present.
    #[error("COUNT and UNTIL cannot be used together")]
    CountAndUntil,
    /// The key does not apply to this frequency.
    #[error("{key} is not allowed with FREQ={frequency}")]
    NotAllowedFor {
        /// The key.
        key: &'static str,
        /// The frequency it was combined with.
        frequency: &'static str,
    },
}

fn invalid(key: &'static str, value: &str) -> RecurrenceError {
    RecurrenceError::InvalidValue {
        key,
        value: value.to_owned(),
    }
}

/// Parses a decimal with no sign, spaces, or leading `+`.
fn digits<T: FromStr>(value: &str) -> Option<T> {
    if value.is_empty() || !value.bytes().all(|b| b.is_ascii_digit()) {
        return None;
    }
    value.parse().ok()
}

fn parse_until(value: &str) -> Result<DateTime<Utc>, RecurrenceError> {
    if value.len() != UNTIL_LEN {
        return Err(invalid("UNTIL", value));
    }
    NaiveDateTime::parse_from_str(value, UNTIL_FORMAT)
        .map(|naive| naive.and_utc())
        .map_err(|_| invalid("UNTIL", value))
}

fn parse_days(value: &str) -> Result<BTreeSet<Weekday>, RecurrenceError> {
    let mut days = BTreeSet::new();
    for part in value.split(',') {
        let day = Weekday::parse(part).ok_or_else(|| invalid("BYDAY", value))?;
        if !days.insert(day) {
            return Err(invalid("BYDAY", value));
        }
    }
    Ok(days)
}

#[derive(Default)]
struct Parts {
    freq: Option<String>,
    interval: Option<u16>,
    days: Option<BTreeSet<Weekday>>,
    month_day: Option<u8>,
    count: Option<NonZeroU32>,
    until: Option<DateTime<Utc>>,
}

/// Stores `value` in `slot`, refusing a second occurrence of `key`.
fn set<T>(slot: &mut Option<T>, key: &str, value: T) -> Result<(), RecurrenceError> {
    if slot.replace(value).is_some() {
        return Err(RecurrenceError::DuplicateKey(key.to_owned()));
    }
    Ok(())
}

impl FromStr for RecurrenceRule {
    type Err = RecurrenceError;

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        if s.is_empty() {
            return Err(RecurrenceError::Empty);
        }
        let mut parts = Parts::default();
        for segment in s.split(';') {
            let (key, value) = segment
                .split_once('=')
                .filter(|(key, value)| !key.is_empty() && !value.is_empty())
                .ok_or_else(|| RecurrenceError::Malformed(segment.to_owned()))?;
            match key {
                "FREQ" => {
                    if !matches!(value, "DAILY" | "WEEKLY" | "MONTHLY") {
                        return Err(RecurrenceError::UnsupportedFrequency(value.to_owned()));
                    }
                    set(&mut parts.freq, key, value.to_owned())?;
                }
                "INTERVAL" => {
                    let n = digits::<u16>(value)
                        .filter(|n| (1..=MAX_INTERVAL).contains(n))
                        .ok_or_else(|| invalid("INTERVAL", value))?;
                    set(&mut parts.interval, key, n)?;
                }
                "BYDAY" => set(&mut parts.days, key, parse_days(value)?)?,
                "BYMONTHDAY" => {
                    let day = digits::<u8>(value)
                        .filter(|d| (1..=31).contains(d))
                        .ok_or_else(|| invalid("BYMONTHDAY", value))?;
                    set(&mut parts.month_day, key, day)?;
                }
                "COUNT" => {
                    let n = digits::<u32>(value)
                        .filter(|n| *n <= MAX_COUNT)
                        .and_then(NonZeroU32::new)
                        .ok_or_else(|| invalid("COUNT", value))?;
                    set(&mut parts.count, key, n)?;
                }
                "UNTIL" => set(&mut parts.until, key, parse_until(value)?)?,
                other => return Err(RecurrenceError::UnsupportedKey(other.to_owned())),
            }
        }
        Self::from_parts(parts)
    }
}

impl RecurrenceRule {
    /// The cross-key checks, once every pair parsed on its own.
    fn from_parts(parts: Parts) -> Result<Self, RecurrenceError> {
        let freq = parts.freq.ok_or(RecurrenceError::MissingKey("FREQ"))?;
        let frequency = match freq.as_str() {
            "DAILY" => Frequency::Daily,
            "WEEKLY" => Frequency::Weekly {
                days: parts
                    .days
                    .clone()
                    .ok_or(RecurrenceError::MissingKey("BYDAY"))?,
            },
            _ => Frequency::Monthly {
                day: parts
                    .month_day
                    .ok_or(RecurrenceError::MissingKey("BYMONTHDAY"))?,
            },
        };
        if parts.days.is_some() && !matches!(frequency, Frequency::Weekly { .. }) {
            return Err(RecurrenceError::NotAllowedFor {
                key: "BYDAY",
                frequency: frequency.name(),
            });
        }
        if parts.month_day.is_some() && !matches!(frequency, Frequency::Monthly { .. }) {
            return Err(RecurrenceError::NotAllowedFor {
                key: "BYMONTHDAY",
                frequency: frequency.name(),
            });
        }
        let end = match (parts.count, parts.until) {
            (Some(_), Some(_)) => return Err(RecurrenceError::CountAndUntil),
            (Some(count), None) => RecurrenceEnd::Count(count),
            (None, Some(until)) => RecurrenceEnd::Until(until),
            (None, None) => RecurrenceEnd::Never,
        };
        Ok(Self {
            frequency,
            interval: parts.interval.unwrap_or(1),
            end,
        })
    }
}

impl fmt::Display for RecurrenceRule {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "FREQ={}", self.frequency.name())?;
        if self.interval > 1 {
            write!(f, ";INTERVAL={}", self.interval)?;
        }
        match &self.frequency {
            Frequency::Daily => {}
            Frequency::Weekly { days } => {
                f.write_str(";BYDAY=")?;
                for (i, day) in days.iter().enumerate() {
                    if i > 0 {
                        f.write_str(",")?;
                    }
                    f.write_str(day.as_str())?;
                }
            }
            Frequency::Monthly { day } => write!(f, ";BYMONTHDAY={day}")?,
        }
        match self.end {
            RecurrenceEnd::Never => Ok(()),
            RecurrenceEnd::Count(count) => write!(f, ";COUNT={count}"),
            RecurrenceEnd::Until(until) => write!(f, ";UNTIL={}", until.format(UNTIL_FORMAT)),
        }
    }
}

impl Serialize for RecurrenceRule {
    fn serialize<S: Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        serializer.collect_str(self)
    }
}

impl<'de> Deserialize<'de> for RecurrenceRule {
    fn deserialize<D: Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
        let text = String::deserialize(deserializer)?;
        text.parse().map_err(serde::de::Error::custom)
    }
}

#[cfg(feature = "openapi")]
impl utoipa::PartialSchema for RecurrenceRule {
    fn schema() -> utoipa::openapi::RefOr<utoipa::openapi::schema::Schema> {
        utoipa::openapi::ObjectBuilder::new()
            .schema_type(utoipa::openapi::schema::Type::String)
            .description(Some(
                "Canonical recurrence rule, e.g. FREQ=WEEKLY;BYDAY=MO,TU;COUNT=10.",
            ))
            .examples([serde_json::json!("FREQ=WEEKLY;BYDAY=MO,TU;COUNT=10")])
            .into()
    }
}

#[cfg(feature = "openapi")]
impl utoipa::ToSchema for RecurrenceRule {}

#[cfg(test)]
#[allow(clippy::unwrap_used)]
mod tests {
    use chrono::TimeZone;
    use proptest::prelude::*;
    use serde_json::json;

    use super::*;

    #[test]
    fn accepts_subset() {
        let cases = [
            ("FREQ=DAILY", "FREQ=DAILY"),
            ("FREQ=WEEKLY;BYDAY=TU,MO", "FREQ=WEEKLY;BYDAY=MO,TU"),
            (
                "FREQ=MONTHLY;BYMONTHDAY=31;COUNT=12",
                "FREQ=MONTHLY;BYMONTHDAY=31;COUNT=12",
            ),
            (
                "FREQ=DAILY;INTERVAL=2;UNTIL=20271231T235959Z",
                "FREQ=DAILY;INTERVAL=2;UNTIL=20271231T235959Z",
            ),
            ("FREQ=WEEKLY;INTERVAL=1;BYDAY=FR", "FREQ=WEEKLY;BYDAY=FR"),
        ];
        for (input, canonical) in cases {
            let rule: RecurrenceRule = input.parse().unwrap();
            assert_eq!(rule.to_string(), canonical, "{input}");
        }
    }

    #[test]
    fn rejects_outside_subset() {
        use RecurrenceError as E;
        let cases: [(&str, E); 18] = [
            ("", E::Empty),
            ("freq=daily", E::UnsupportedKey("freq".into())),
            ("FREQ=DAILY;", E::Malformed(String::new())),
            ("RRULE:FREQ=DAILY", E::UnsupportedKey("RRULE:FREQ".into())),
            ("FREQ=YEARLY", E::UnsupportedFrequency("YEARLY".into())),
            ("FREQ=HOURLY", E::UnsupportedFrequency("HOURLY".into())),
            ("FREQ=WEEKLY", E::MissingKey("BYDAY")),
            (
                "FREQ=WEEKLY;BYDAY=1MO",
                E::InvalidValue {
                    key: "BYDAY",
                    value: "1MO".into(),
                },
            ),
            (
                "FREQ=WEEKLY;BYDAY=MO,MO",
                E::InvalidValue {
                    key: "BYDAY",
                    value: "MO,MO".into(),
                },
            ),
            (
                "FREQ=DAILY;BYDAY=MO",
                E::NotAllowedFor {
                    key: "BYDAY",
                    frequency: "DAILY",
                },
            ),
            (
                "FREQ=MONTHLY;BYMONTHDAY=-1",
                E::InvalidValue {
                    key: "BYMONTHDAY",
                    value: "-1".into(),
                },
            ),
            (
                "FREQ=MONTHLY;BYMONTHDAY=1,15",
                E::InvalidValue {
                    key: "BYMONTHDAY",
                    value: "1,15".into(),
                },
            ),
            (
                "FREQ=DAILY;COUNT=3;UNTIL=20271231T000000Z",
                E::CountAndUntil,
            ),
            (
                "FREQ=DAILY;UNTIL=20271231",
                E::InvalidValue {
                    key: "UNTIL",
                    value: "20271231".into(),
                },
            ),
            (
                "FREQ=DAILY;INTERVAL=0",
                E::InvalidValue {
                    key: "INTERVAL",
                    value: "0".into(),
                },
            ),
            ("FREQ=DAILY;BYHOUR=9", E::UnsupportedKey("BYHOUR".into())),
            ("FREQ=DAILY;WKST=MO", E::UnsupportedKey("WKST".into())),
            ("FREQ=DAILY;FREQ=WEEKLY", E::DuplicateKey("FREQ".into())),
        ];
        for (input, want) in cases {
            assert_eq!(input.parse::<RecurrenceRule>(), Err(want), "{input:?}");
        }
    }

    #[test]
    fn limits_are_inclusive() {
        for ok in ["FREQ=DAILY;INTERVAL=365", "FREQ=DAILY;COUNT=1000"] {
            assert!(ok.parse::<RecurrenceRule>().is_ok(), "{ok}");
        }
        for bad in [
            "FREQ=DAILY;INTERVAL=366",
            "FREQ=DAILY;COUNT=1001",
            "FREQ=DAILY;COUNT=0",
            "FREQ=MONTHLY;BYMONTHDAY=32",
            "FREQ=MONTHLY;BYMONTHDAY=0",
            "FREQ=DAILY;INTERVAL=+2",
        ] {
            assert!(bad.parse::<RecurrenceRule>().is_err(), "{bad}");
        }
    }

    #[test]
    fn serde_uses_canonical_string() {
        let rule: RecurrenceRule = "FREQ=WEEKLY;BYDAY=MO,WE;COUNT=4".parse().unwrap();
        let value = serde_json::to_value(&rule).unwrap();
        assert_eq!(value, json!("FREQ=WEEKLY;BYDAY=MO,WE;COUNT=4"));
        assert_eq!(
            serde_json::from_value::<RecurrenceRule>(value).unwrap(),
            rule
        );
        assert!(serde_json::from_value::<RecurrenceRule>(json!("freq=daily")).is_err());
    }

    fn rule_strategy() -> impl Strategy<Value = RecurrenceRule> {
        let days = proptest::collection::btree_set(
            prop_oneof![
                Just(Weekday::Mo),
                Just(Weekday::Tu),
                Just(Weekday::We),
                Just(Weekday::Th),
                Just(Weekday::Fr),
                Just(Weekday::Sa),
                Just(Weekday::Su),
            ],
            1..=7,
        );
        let frequency = prop_oneof![
            Just(Frequency::Daily),
            days.prop_map(|days| Frequency::Weekly { days }),
            (1u8..=31).prop_map(|day| Frequency::Monthly { day }),
        ];
        let end = prop_oneof![
            Just(RecurrenceEnd::Never),
            (1u32..=MAX_COUNT).prop_map(|n| RecurrenceEnd::Count(NonZeroU32::new(n).unwrap())),
            (0i64..4_000_000_000)
                .prop_map(|secs| { RecurrenceEnd::Until(Utc.timestamp_opt(secs, 0).unwrap()) }),
        ];
        (frequency, 1..=MAX_INTERVAL, end).prop_map(|(frequency, interval, end)| RecurrenceRule {
            frequency,
            interval,
            end,
        })
    }

    proptest! {
        #[test]
        fn display_and_parse_round_trip(rule in rule_strategy()) {
            let text = rule.to_string();
            prop_assert_eq!(text.parse::<RecurrenceRule>().unwrap(), rule);
        }
    }
}
