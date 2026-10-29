//! The per-post options that script the mock without code: `outcome`, `comments`, and
//! `reconcile`.

use postit_agent_kit::message;
use postit_core::{PlatformId, Violation};
use serde_json::{Map, Value};

/// What the mock platform does with a live publish, picked by the `outcome` option.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Outcome {
    /// The post goes live.
    Published,
    /// The post is accepted and goes live at the first `status` call.
    Processing,
    /// The platform fails with 503 before acting; retrying is safe.
    RetryableFailure,
    /// The platform refuses the post with 400.
    PermanentFailure,
    /// The platform stores the post, then the response is lost.
    OutcomeUnknownLanded,
    /// The request is lost before the platform acts.
    OutcomeUnknownLost,
    /// The platform rejects the token.
    NeedsReconnect,
    /// The platform asks the caller to wait 30 seconds.
    RateLimited,
    /// The daily `posts` quota is used up.
    QuotaExhausted,
}

impl Outcome {
    /// Every outcome, in the order the options form lists them.
    pub const ALL: [Self; 9] = [
        Self::Published,
        Self::Processing,
        Self::RetryableFailure,
        Self::PermanentFailure,
        Self::OutcomeUnknownLanded,
        Self::OutcomeUnknownLost,
        Self::NeedsReconnect,
        Self::RateLimited,
        Self::QuotaExhausted,
    ];

    /// The option's wire value.
    #[must_use]
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Published => "published",
            Self::Processing => "processing",
            Self::RetryableFailure => "retryable_failure",
            Self::PermanentFailure => "permanent_failure",
            Self::OutcomeUnknownLanded => "outcome_unknown_landed",
            Self::OutcomeUnknownLost => "outcome_unknown_lost",
            Self::NeedsReconnect => "needs_reconnect",
            Self::RateLimited => "rate_limited",
            Self::QuotaExhausted => "quota_exhausted",
        }
    }

    /// The English label in the options form.
    #[must_use]
    pub const fn label(self) -> &'static str {
        match self {
            Self::Published => "Published",
            Self::Processing => "Processing, then published",
            Self::RetryableFailure => "Temporary failure",
            Self::PermanentFailure => "Permanent failure",
            Self::OutcomeUnknownLanded => "Response lost after posting",
            Self::OutcomeUnknownLost => "Request lost before posting",
            Self::NeedsReconnect => "Needs reconnect",
            Self::RateLimited => "Rate limited",
            Self::QuotaExhausted => "Quota exhausted",
        }
    }
}

/// What `reconcile` (and `reconcile_comment`) answer, picked by the `reconcile` option.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ReconcileAnswer {
    /// Look the item up under its idempotency key: `Found` when the earlier call landed,
    /// `Absent` otherwise. The default.
    Found,
    /// `Absent`, whatever the platform holds.
    Absent,
    /// `ReconcileOutcome::Unresolvable`.
    Unresolvable,
    /// `AgentError::Unsupported`: the platform has no way to look the item up.
    Unsupported,
}

impl ReconcileAnswer {
    /// Every answer, in the order the options form lists them.
    pub const ALL: [Self; 4] = [
        Self::Found,
        Self::Absent,
        Self::Unresolvable,
        Self::Unsupported,
    ];

    /// The option's wire value.
    #[must_use]
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Found => "found",
            Self::Absent => "absent",
            Self::Unresolvable => "unresolvable",
            Self::Unsupported => "unsupported",
        }
    }

    /// The English label in the options form.
    #[must_use]
    pub const fn label(self) -> &'static str {
        match self {
            Self::Found => "Found when it landed",
            Self::Absent => "Absent",
            Self::Unresolvable => "Cannot tell",
            Self::Unsupported => "Lookup not supported",
        }
    }
}

/// A variant's mock options, with the schema defaults filled in.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) struct PostOptions {
    pub outcome: Outcome,
    pub comments: u32,
    pub reconcile: ReconcileAnswer,
}

impl PostOptions {
    /// Reads `options`. A value the schema would refuse is a violation, so a variant that
    /// skipped the engine's checks still fails before anything is sent.
    pub(crate) fn parse(
        platform: &PlatformId,
        options: &Map<String, Value>,
    ) -> Result<Self, Vec<Violation>> {
        let mut violations = Vec::new();
        let outcome = match options.get("outcome") {
            None | Some(Value::Null) => Outcome::Published,
            Some(value) => value
                .as_str()
                .and_then(|v| Outcome::ALL.into_iter().find(|o| o.as_str() == v))
                .unwrap_or_else(|| {
                    violations.push(rule(
                        platform,
                        "invalid_outcome",
                        "Unknown outcome",
                        "outcome",
                    ));
                    Outcome::Published
                }),
        };
        let comments = match options.get("comments") {
            None | Some(Value::Null) => 0,
            Some(value) => value
                .as_i64()
                .filter(|n| (0..=crate::descriptor::MAX_SEEDED_COMMENTS).contains(n))
                .and_then(|n| u32::try_from(n).ok())
                .unwrap_or_else(|| {
                    violations.push(rule(
                        platform,
                        "invalid_comments",
                        "Comments to seed must be between 0 and 20",
                        "comments",
                    ));
                    0
                }),
        };
        let reconcile = match options.get("reconcile") {
            None | Some(Value::Null) => ReconcileAnswer::Found,
            Some(value) => value
                .as_str()
                .and_then(|v| ReconcileAnswer::ALL.into_iter().find(|r| r.as_str() == v))
                .unwrap_or_else(|| {
                    violations.push(rule(
                        platform,
                        "invalid_reconcile",
                        "Unknown reconcile answer",
                        "reconcile",
                    ));
                    ReconcileAnswer::Found
                }),
        };
        if violations.is_empty() {
            Ok(Self {
                outcome,
                comments,
                reconcile,
            })
        } else {
            Err(violations)
        }
    }
}

fn rule(platform: &PlatformId, key: &str, en: &str, option: &str) -> Violation {
    Violation::agent_rule(message(platform, &format!("options.{key}"), en))
        .at(format!("options.{option}"))
}
