//! Schedule input: an instant with an offset, or a local time with an IANA zone.

use std::str::FromStr;

use chrono::{DateTime, FixedOffset, LocalResult, NaiveDateTime, Offset, TimeZone, Utc};
use chrono_tz::Tz;
use serde::{Deserialize, Deserializer, Serialize};

use super::{Violation, ViolationCode};

/// When a post goes out, as a client sends it.
///
/// A string is RFC 3339 *with* an offset (`2026-11-08T09:00:00+02:00`); a string without an
/// offset does not parse. An object is a local date-time (seconds optional) and an optional
/// IANA zone; without a zone the caller's default applies.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "openapi", derive(utoipa::ToSchema))]
#[serde(untagged)]
pub enum ScheduleInput {
    /// An exact instant.
    Instant(DateTime<FixedOffset>),
    /// A wall-clock time in a zone.
    Local {
        /// `YYYY-MM-DDTHH:MM[:SS]`.
        #[serde(deserialize_with = "deserialize_local")]
        #[cfg_attr(
            feature = "openapi",
            schema(value_type = String, example = "2026-11-08T09:00")
        )]
        local: NaiveDateTime,
        /// IANA zone name such as `Europe/Berlin`; the caller's own zone when absent.
        #[serde(default, skip_serializing_if = "Option::is_none")]
        zone: Option<String>,
    },
}

fn deserialize_local<'de, D: Deserializer<'de>>(d: D) -> Result<NaiveDateTime, D::Error> {
    let s = String::deserialize(d)?;
    NaiveDateTime::parse_from_str(&s, "%Y-%m-%dT%H:%M:%S")
        .or_else(|_| NaiveDateTime::parse_from_str(&s, "%Y-%m-%dT%H:%M"))
        .map_err(serde::de::Error::custom)
}

/// Parses an IANA zone name. The one function that decides what a valid zone is.
#[must_use]
pub fn parse_zone(zone: &str) -> Option<Tz> {
    Tz::from_str(zone).ok()
}

fn offset_text(offset: FixedOffset) -> String {
    let secs = offset.local_minus_utc();
    let sign = if secs < 0 { '-' } else { '+' };
    let abs = secs.abs();
    format!("{sign}{:02}:{:02}", abs / 3600, (abs % 3600) / 60)
}

/// Resolves a schedule input to UTC.
///
/// # Errors
/// A `schedule` violation: unknown zone, a local time inside a DST gap, or one that happens
/// twice in a DST overlap.
pub fn resolve_schedule(
    input: &ScheduleInput,
    default_zone: &str,
) -> Result<DateTime<Utc>, Violation> {
    match input {
        ScheduleInput::Instant(at) => Ok(at.with_timezone(&Utc)),
        ScheduleInput::Local { local, zone } => {
            let name = zone.as_deref().unwrap_or(default_zone);
            let tz = parse_zone(name).ok_or_else(|| {
                Violation::new(ViolationCode::ScheduleZoneUnknown)
                    .at("schedule")
                    .param("zone", name)
            })?;
            let local_text = local.format("%Y-%m-%dT%H:%M:%S").to_string();
            match tz.from_local_datetime(local) {
                LocalResult::Single(at) => Ok(at.with_timezone(&Utc)),
                LocalResult::None => Err(Violation::new(ViolationCode::ScheduleTimeNonexistent)
                    .at("schedule")
                    .param("local", local_text)
                    .param("zone", name)),
                LocalResult::Ambiguous(first, second) => {
                    Err(Violation::new(ViolationCode::ScheduleTimeAmbiguous)
                        .at("schedule")
                        .param("local", local_text)
                        .param("zone", name)
                        .param("earliest_offset", offset_text(first.offset().fix()))
                        .param("latest_offset", offset_text(second.offset().fix())))
                }
            }
        }
    }
}

#[cfg(test)]
#[allow(clippy::unwrap_used)]
mod tests {
    use super::*;

    fn local(text: &str, zone: Option<&str>) -> ScheduleInput {
        serde_json::from_value(serde_json::json!({"local": text, "zone": zone})).unwrap()
    }

    fn utc(text: &str) -> DateTime<Utc> {
        text.parse().unwrap()
    }

    #[test]
    fn resolve_instant_keeps_the_offset() {
        let input: ScheduleInput = serde_json::from_str("\"2026-11-08T09:00:00+02:00\"").unwrap();
        assert_eq!(
            resolve_schedule(&input, "UTC").unwrap(),
            utc("2026-11-08T07:00:00Z")
        );
    }

    #[test]
    fn string_without_offset_does_not_parse() {
        assert!(serde_json::from_str::<ScheduleInput>("\"2026-11-08T09:00:00\"").is_err());
    }

    #[test]
    fn resolve_local_with_zone() {
        let input = local("2026-11-08T09:00", Some("Europe/Berlin"));
        assert_eq!(
            resolve_schedule(&input, "UTC").unwrap(),
            utc("2026-11-08T08:00:00Z")
        );
    }

    #[test]
    fn resolve_local_without_zone_uses_default() {
        let input: ScheduleInput = serde_json::from_str(r#"{"local":"2026-11-08T09:00"}"#).unwrap();
        assert_eq!(
            resolve_schedule(&input, "UTC").unwrap(),
            utc("2026-11-08T09:00:00Z")
        );
    }

    #[test]
    fn dst_gap_is_nonexistent() {
        let v =
            resolve_schedule(&local("2026-03-29T02:30", Some("Europe/Berlin")), "UTC").unwrap_err();
        assert_eq!(v.code, ViolationCode::ScheduleTimeNonexistent);
        assert_eq!(v.path.as_deref(), Some("schedule"));
        assert!(v.params.contains_key("local"));
        assert_eq!(v.params["zone"], "Europe/Berlin");
    }

    #[test]
    fn dst_overlap_is_ambiguous() {
        let v =
            resolve_schedule(&local("2026-10-25T02:30", Some("Europe/Berlin")), "UTC").unwrap_err();
        assert_eq!(v.code, ViolationCode::ScheduleTimeAmbiguous);
        assert_eq!(v.params["earliest_offset"], "+02:00");
        assert_eq!(v.params["latest_offset"], "+01:00");
    }

    #[test]
    fn unknown_zone() {
        let v =
            resolve_schedule(&local("2026-11-08T09:00", Some("Mars/Olympus")), "UTC").unwrap_err();
        assert_eq!(v.code, ViolationCode::ScheduleZoneUnknown);
        assert_eq!(v.params["zone"], "Mars/Olympus");
    }

    #[test]
    fn violation_codes_are_listed() {
        for wire in [
            "schedule_time_nonexistent",
            "schedule_time_ambiguous",
            "schedule_zone_unknown",
        ] {
            assert!(
                ViolationCode::ALL.iter().any(|c| c.as_str() == wire),
                "{wire}"
            );
        }
    }
}
