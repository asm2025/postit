//! What the mock tells the system about itself: platform ID, formats, capabilities, schedule
//! window, quota, the credentials form, and the per-post options.
//!
//! A real agent's descriptor module looks the same: constants for the platform's limits, then
//! one function building the [`AgentDescriptor`]. Every human-facing string is a
//! [`message`] keyed `<platform>.…` with English text.

use std::collections::BTreeSet;
use std::time::Duration;

use postit_agent_kit::{media_format, message, schema, text_format, text_rules};
use postit_core::{
    AgentDescriptor, AgentKind, Capability, ConnectMethod, FormatId, FormatSpec, MediaKind,
    MediaRules, NativeScheduleWindow, OAuthProviderSpec, OptionsSchema, PlatformId, QuotaBucket,
    QuotaCosts, QuotaPeriod, QuotaScope, TextMeasure,
};
use serde_json::{Value, json};
use url::Url;

use crate::options::Outcome;

/// The broadcast mock's platform ID.
pub const MOCK: &str = "mock";
/// The direct-send mock's platform ID.
pub const MOCK_DIRECT: &str = "mock_direct";
/// The OAuth mock's platform ID.
pub const MOCK_OAUTH: &str = "mock_oauth";

/// The quota bucket every live publish counts against.
pub const POSTS_BUCKET: &str = "posts";

/// Longest post text, in graphemes.
pub const MAX_TEXT: u32 = 500;
/// Longest direct message text, in graphemes.
pub const MAX_DIRECT_TEXT: u32 = 20_000;
/// Most comments the `comments` option may seed.
pub const MAX_SEEDED_COMMENTS: i64 = 20;
/// Shortest lead time of a native schedule.
pub const MIN_SCHEDULE_LEAD: Duration = Duration::from_mins(10);
/// Longest lead time of a native schedule.
pub const MAX_SCHEDULE_LEAD: Duration = Duration::from_hours(30 * 24);

/// A 1x1 PNG in the mock's brand color.
static ICON: &[u8] = &[
    0x89, 0x50, 0x4e, 0x47, 0x0d, 0x0a, 0x1a, 0x0a, 0x00, 0x00, 0x00, 0x0d, 0x49, 0x48, 0x44, 0x52,
    0x00, 0x00, 0x00, 0x01, 0x00, 0x00, 0x00, 0x01, 0x08, 0x02, 0x00, 0x00, 0x00, 0x90, 0x77, 0x53,
    0xde, 0x00, 0x00, 0x00, 0x0c, 0x49, 0x44, 0x41, 0x54, 0x78, 0x9c, 0x63, 0xc8, 0x2e, 0x6a, 0x00,
    0x00, 0x02, 0xa9, 0x01, 0x5e, 0xa8, 0x6a, 0xec, 0xac, 0x00, 0x00, 0x00, 0x00, 0x49, 0x45, 0x4e,
    0x44, 0xae, 0x42, 0x60, 0x82,
];
const BRAND_COLOR: &str = "#6B7280";

fn platform_id(id: &str) -> PlatformId {
    PlatformId::new(id).unwrap_or_else(|e| unreachable!("the mock's platform ID is valid: {e}"))
}

/// Parses a schema the mock builds from constants; a failure is a bug the tests catch.
fn parse(platform: &PlatformId, json: &Value) -> OptionsSchema {
    schema(platform, &json.to_string()).unwrap_or_else(|e| unreachable!("{e}"))
}

/// The credentials form: an account name (or several, comma-separated) and a secret token.
#[must_use]
pub fn credentials_schema(platform: &PlatformId) -> OptionsSchema {
    let m = |key: &str, en: &str| message(platform, key, en);
    parse(
        platform,
        &json!({
            "type": "object",
            "properties": {
                "account_name": {
                    "type": "string",
                    "minLength": 1,
                    "maxLength": 200,
                    "x-postit-label": m("credentials.account_name.label", "Account name"),
                    "x-postit-help": m(
                        "credentials.account_name.help",
                        "Separate several names with commas to connect several accounts.",
                    ),
                },
                "token": {
                    "type": "string",
                    "minLength": 1,
                    "maxLength": 200,
                    "x-postit-secret": true,
                    "x-postit-label": m("credentials.token.label", "Token"),
                    "x-postit-help": m(
                        "credentials.token.help",
                        "Type invalid, rate_limited, or expired to test those results.",
                    ),
                },
            },
            "required": ["account_name", "token"],
            "additionalProperties": false,
        }),
    )
}

/// The per-post options: which `outcome` the mock platform answers with, and how many
/// `comments` to seed on the new post.
#[must_use]
pub fn options_schema(platform: &PlatformId) -> OptionsSchema {
    let m = |key: &str, en: &str| message(platform, key, en);
    let values: Vec<&str> = Outcome::ALL.iter().map(|o| o.as_str()).collect();
    let labels: Vec<_> = Outcome::ALL
        .iter()
        .map(|o| m(&format!("options.outcome.{}", o.as_str()), o.label()))
        .collect();
    parse(
        platform,
        &json!({
            "type": "object",
            "properties": {
                "outcome": {
                    "type": "string",
                    "enum": values,
                    "x-postit-enum-labels": labels,
                    "default": Outcome::Published.as_str(),
                    "x-postit-label": m("options.outcome.label", "Outcome"),
                    "x-postit-help": m(
                        "options.outcome.help",
                        "What the mock platform does with this post.",
                    ),
                },
                "comments": {
                    "type": "integer",
                    "minimum": 0,
                    "maximum": MAX_SEEDED_COMMENTS,
                    "default": 0,
                    "x-postit-label": m("options.comments.label", "Comments to seed"),
                },
            },
            "additionalProperties": false,
        }),
    )
}

fn media(kind: MediaKind, min_count: u32, max_count: u32, mime_types: &[&str]) -> MediaRules {
    MediaRules {
        min_count,
        max_count,
        kinds: vec![kind],
        mime_types: mime_types.iter().map(|&m| m.to_owned()).collect(),
        ..MediaRules::none()
    }
}

fn broadcast_formats() -> Vec<FormatSpec> {
    let caption = text_rules(TextMeasure::Graphemes, MAX_TEXT, false);
    vec![
        text_format(TextMeasure::Graphemes, MAX_TEXT),
        media_format(
            FormatId::Image,
            caption,
            media(MediaKind::Image, 1, 4, &["image/jpeg", "image/png"]),
        ),
        media_format(
            FormatId::Video,
            caption,
            media(MediaKind::Video, 1, 1, &["video/mp4"]),
        ),
    ]
}

fn base(platform: &str, display_name: &str, connect: ConnectMethod) -> AgentDescriptor {
    let id = platform_id(platform);
    AgentDescriptor {
        options_schema: options_schema(&id),
        platform: id,
        display_name: display_name.to_owned(),
        icon_png: ICON,
        brand_color: BRAND_COLOR.to_owned(),
        kind: AgentKind::Broadcast,
        connect,
        formats: broadcast_formats(),
        capabilities: BTreeSet::from([
            Capability::Commenter,
            Capability::InboxReader,
            Capability::MetricsReader,
            Capability::NativeScheduler,
        ]),
        config_schema_version: 1,
        native_schedule: Some(NativeScheduleWindow {
            min_lead_secs: MIN_SCHEDULE_LEAD.as_secs(),
            max_lead_secs: MAX_SCHEDULE_LEAD.as_secs(),
        }),
        quota_buckets: vec![QuotaBucket {
            id: POSTS_BUCKET.to_owned(),
            scope: QuotaScope::Account,
            period: QuotaPeriod::Day,
            default_limit: 50,
            reset_time_zone: "UTC".to_owned(),
            costs: QuotaCosts {
                publish: 1,
                comment: 0,
                inbox_poll: 0,
                metrics: 0,
            },
        }],
    }
}

/// `mock`: a broadcast platform connected with pasted credentials, with every broadcast
/// capability.
#[must_use]
pub fn broadcast() -> AgentDescriptor {
    let connect = ConnectMethod::Credentials(credentials_schema(&platform_id(MOCK)));
    base(MOCK, "Mock", connect)
}

/// `mock_direct`: sends to individual recipients, connected with the same credentials form.
#[must_use]
pub fn direct() -> AgentDescriptor {
    let connect = ConnectMethod::Credentials(credentials_schema(&platform_id(MOCK_DIRECT)));
    AgentDescriptor {
        kind: AgentKind::Direct,
        formats: vec![media_format(
            FormatId::HtmlEmail,
            text_rules(TextMeasure::Graphemes, MAX_DIRECT_TEXT, false),
            MediaRules::none(),
        )],
        capabilities: BTreeSet::from([Capability::DirectSender]),
        native_schedule: None,
        ..base(MOCK_DIRECT, "Mock direct", connect)
    }
}

/// `mock_oauth`: the broadcast mock connected through the OAuth provider at `provider`
/// (`authorize`, `token`, and `revoke` under it, scopes `read` and `write`).
#[must_use]
pub fn oauth(provider: &Url) -> AgentDescriptor {
    let join = |path: &str| {
        provider
            .join(path)
            .unwrap_or_else(|e| unreachable!("a fixed relative path joins: {e}"))
    };
    let spec = OAuthProviderSpec::new(join("authorize"), join("token"))
        .with_revoke_url(join("revoke"))
        .with_scopes(["read", "write"]);
    base(MOCK_OAUTH, "Mock OAuth", ConnectMethod::OAuth(spec))
}
