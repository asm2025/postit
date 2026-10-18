//! Engagement numbers read from a platform. A missing number means the platform does not
//! report it, which is different from zero.

use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};

/// Engagement on one post at one moment.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "openapi", derive(utoipa::ToSchema))]
pub struct MetricSnapshot {
    /// Times the post was seen.
    pub views: Option<u64>,
    /// Likes or reactions.
    pub likes: Option<u64>,
    /// Comments.
    pub comments: Option<u64>,
    /// Shares or reposts.
    pub shares: Option<u64>,
    /// Saves or bookmarks.
    pub saves: Option<u64>,
    /// Link clicks.
    pub clicks: Option<u64>,
    /// When the numbers were read.
    pub collected_at: DateTime<Utc>,
}

/// Numbers about the account itself.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "openapi", derive(utoipa::ToSchema))]
pub struct AccountMetrics {
    /// Followers or subscribers.
    pub followers: Option<u64>,
    /// People reached over the platform's reporting period.
    pub reach: Option<u64>,
}

#[cfg(test)]
#[allow(clippy::unwrap_used)]
mod tests {
    use chrono::TimeZone;

    use super::*;

    #[test]
    fn metric_snapshot_round_trips_and_keeps_nulls() {
        let snap = MetricSnapshot {
            views: Some(10),
            likes: None,
            comments: Some(0),
            shares: None,
            saves: None,
            clicks: None,
            collected_at: Utc.with_ymd_and_hms(2026, 10, 18, 12, 0, 0).unwrap(),
        };
        let json = serde_json::to_value(snap).unwrap();
        assert_eq!(json["likes"], serde_json::Value::Null);
        assert_eq!(json["comments"], 0);
        assert_eq!(
            serde_json::from_value::<MetricSnapshot>(json).unwrap(),
            snap
        );
    }
}
