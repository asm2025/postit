//! What an agent returns after publishing, reconciling, or connecting an account.

use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use url::Url;

/// The platform's record of a published (or accepted) post.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "openapi", derive(utoipa::ToSchema))]
pub struct PublishReceipt {
    /// The platform's ID for the post.
    pub remote_id: Option<String>,
    /// A link to the post.
    pub remote_url: Option<Url>,
    /// An ID for a container or upload the platform is still processing.
    pub container_ref: Option<String>,
    /// When the platform will publish a natively scheduled post.
    pub remote_publish_at: Option<DateTime<Utc>>,
}

/// What a publish call achieved.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "openapi", derive(utoipa::ToSchema))]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum PublishOutcome {
    /// The post is live.
    Published(PublishReceipt),
    /// The platform accepted the post and is still processing it.
    Processing(PublishReceipt),
    /// A dry run: nothing was sent. Lists what would have been.
    Simulated {
        /// Human-readable steps the agent would have taken.
        planned: Vec<String>,
    },
}

/// What reconciling an ambiguous delivery found.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "openapi", derive(utoipa::ToSchema))]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum ReconcileOutcome {
    /// The post exists on the platform.
    Found(PublishReceipt),
    /// The platform has no such post; it is safe to publish again.
    Absent,
    /// The agent cannot tell; the user must decide.
    Unresolvable,
}

/// Whether an account's connection works.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[cfg_attr(feature = "openapi", derive(utoipa::ToSchema))]
#[serde(rename_all = "snake_case")]
pub enum AccountStatus {
    /// Working.
    Active,
    /// The user must reconnect.
    NeedsReconnect,
    /// The user revoked access at the platform.
    Revoked,
}

/// An account as the platform describes it.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "openapi", derive(utoipa::ToSchema))]
pub struct AccountInfo {
    /// The platform's ID for the account.
    pub remote_id: String,
    /// Name shown to people.
    pub display_name: String,
    /// The `@handle`, when the platform has one.
    pub handle: Option<String>,
    /// Profile picture.
    pub avatar_url: Option<Url>,
    /// Whether the connection works.
    pub status: AccountStatus,
}

/// An account the user could connect from a successful sign-in.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "openapi", derive(utoipa::ToSchema))]
pub struct DiscoveredAccount {
    /// The platform's ID for the account.
    pub remote_id: String,
    /// Name shown to people.
    pub display_name: String,
    /// The `@handle`, when the platform has one.
    pub handle: Option<String>,
    /// Profile picture.
    pub avatar_url: Option<Url>,
}

/// The accounts a sign-in gave access to.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "openapi", derive(utoipa::ToSchema))]
pub struct Discovered {
    /// Accounts the user may pick from.
    pub accounts: Vec<DiscoveredAccount>,
}

#[cfg(test)]
#[allow(clippy::unwrap_used)]
mod tests {
    use serde_json::json;

    use super::*;

    #[test]
    fn outcome_wire_format() {
        let outcome = PublishOutcome::Processing(PublishReceipt {
            remote_id: Some("v1".to_owned()),
            remote_url: None,
            container_ref: Some("c1".to_owned()),
            remote_publish_at: None,
        });
        let want = json!({"type":"processing","remote_id":"v1","remote_url":null,"container_ref":"c1","remote_publish_at":null});
        assert_eq!(serde_json::to_value(&outcome).unwrap(), want);
        assert_eq!(
            serde_json::from_value::<PublishOutcome>(want).unwrap(),
            outcome
        );
        assert_eq!(
            serde_json::to_value(PublishOutcome::Simulated {
                planned: vec!["post".to_owned()]
            })
            .unwrap(),
            json!({"type":"simulated","planned":["post"]})
        );
        assert_eq!(
            serde_json::to_value(ReconcileOutcome::Absent).unwrap(),
            json!({"type":"absent"})
        );
    }

    #[test]
    fn account_info_round_trips() {
        let info = AccountInfo {
            remote_id: "1".to_owned(),
            display_name: "Demo".to_owned(),
            handle: Some("@demo".to_owned()),
            avatar_url: Some("https://example.com/a.png".parse().unwrap()),
            status: AccountStatus::NeedsReconnect,
        };
        let json = serde_json::to_value(&info).unwrap();
        assert_eq!(json["status"], "needs_reconnect");
        assert_eq!(serde_json::from_value::<AccountInfo>(json).unwrap(), info);
    }
}
