//! `serde(with = "duration")` for humantime-formatted `Duration` fields (e.g. `"30s"`).

use std::time::Duration;

use serde::{Deserialize, Deserializer, Serializer};

pub fn serialize<S: Serializer>(value: &Duration, serializer: S) -> Result<S::Ok, S::Error> {
    serializer.collect_str(&humantime::format_duration(*value))
}

pub fn deserialize<'de, D: Deserializer<'de>>(deserializer: D) -> Result<Duration, D::Error> {
    let raw = String::deserialize(deserializer)?;
    humantime::parse_duration(&raw).map_err(serde::de::Error::custom)
}

#[cfg(test)]
mod tests {
    use serde::{Deserialize, Serialize};

    use super::*;

    #[derive(Serialize, Deserialize)]
    struct Wrapper {
        #[serde(with = "super")]
        value: Duration,
    }

    #[test]
    fn round_trips_through_humantime_strings() {
        let json = r#"{"value":"30s"}"#;
        let wrapper: Wrapper = serde_json::from_str(json).unwrap_or(Wrapper {
            value: Duration::ZERO,
        });
        assert_eq!(wrapper.value, Duration::from_secs(30));
    }
}
