//! What the email agent tells the system about itself: platform ID, format, capabilities,
//! the credentials form, and the per-campaign options.

use std::collections::BTreeSet;

use postit_agent_kit::{media_format, message, schema, text_rules};
use postit_core::{
    AgentDescriptor, AgentKind, Capability, ConnectMethod, FormatId, MediaKind, MediaRules,
    OptionsSchema, PlatformId, TextMeasure,
};
use serde_json::{Value, json};

/// The email agent's platform ID.
pub const EMAIL: &str = "email";

/// The largest HTML part a campaign may render to, every link at its signed length. Gmail
/// clips messages above about 102 KB.
pub const MAX_HTML_BYTES: usize = 100 * 1024;

/// Longest post text, in graphemes. The document, not the text, is what is sent.
const MAX_TEXT: u32 = 20_000;

/// A 1x1 PNG.
static ICON: &[u8] = &[
    0x89, 0x50, 0x4e, 0x47, 0x0d, 0x0a, 0x1a, 0x0a, 0x00, 0x00, 0x00, 0x0d, 0x49, 0x48, 0x44, 0x52,
    0x00, 0x00, 0x00, 0x01, 0x00, 0x00, 0x00, 0x01, 0x08, 0x02, 0x00, 0x00, 0x00, 0x90, 0x77, 0x53,
    0xde, 0x00, 0x00, 0x00, 0x0c, 0x49, 0x44, 0x41, 0x54, 0x78, 0x9c, 0x63, 0xc8, 0x2e, 0x6a, 0x00,
    0x00, 0x02, 0xa9, 0x01, 0x5e, 0xa8, 0x6a, 0xec, 0xac, 0x00, 0x00, 0x00, 0x00, 0x49, 0x45, 0x4e,
    0x44, 0xae, 0x42, 0x60, 0x82,
];
const BRAND_COLOR: &str = "#2563EB";

fn platform_id() -> PlatformId {
    PlatformId::new(EMAIL).unwrap_or_else(|e| unreachable!("the email platform ID is valid: {e}"))
}

/// Parses a schema built from constants; a failure is a bug the tests catch.
fn parse(platform: &PlatformId, json: &Value) -> OptionsSchema {
    schema(platform, &json.to_string()).unwrap_or_else(|e| unreachable!("{e}"))
}

/// Parses an object schema whose properties keep the order given, which the form shows them
/// in and `x-postit-depends-on` needs (`serde_json` maps sort their keys).
fn parse_ordered(
    platform: &PlatformId,
    properties: &[(&str, Value)],
    required: &[&str],
) -> OptionsSchema {
    let properties: Vec<String> = properties
        .iter()
        .map(|(name, property)| format!("{}:{property}", Value::from(*name)))
        .collect();
    let text = format!(
        "{{\"type\":\"object\",\"properties\":{{{}}},\"required\":{},\"additionalProperties\":false}}",
        properties.join(","),
        json!(required),
    );
    schema(platform, &text).unwrap_or_else(|e| unreachable!("{e}"))
}

/// The credentials form: the sender, the postal address for the footer, and the SMTP server.
#[must_use]
pub fn credentials_schema(platform: &PlatformId) -> OptionsSchema {
    let mut properties = sender_properties(platform);
    properties.extend(smtp_properties(platform));
    parse_ordered(platform, &properties, &["from_address", "host"])
}

/// The sending method, who the mail comes from, and the footer's postal address.
fn sender_properties(platform: &PlatformId) -> Vec<(&'static str, Value)> {
    let m = |key: &str, en: &str| message(platform, key, en);
    vec![
        (
            "transport",
            json!({
                "type": "string",
                "enum": ["smtp"],
                "x-postit-enum-labels": [m("credentials.transport.smtp", "SMTP server")],
                "default": "smtp",
                "x-postit-label": m("credentials.transport.label", "Sending method"),
            }),
        ),
        (
            "from_name",
            json!({
                "type": "string",
                "maxLength": 200,
                "x-postit-label": m("credentials.from_name.label", "From name"),
            }),
        ),
        (
            "from_address",
            json!({
                "type": "string",
                "minLength": 3,
                "maxLength": 254,
                "x-postit-label": m("credentials.from_address.label", "From address"),
            }),
        ),
        (
            "reply_to",
            json!({
                "type": "string",
                "maxLength": 254,
                "x-postit-label": m("credentials.reply_to.label", "Reply-to address"),
            }),
        ),
        (
            "postal_address",
            json!({
                "type": "string",
                "maxLength": 500,
                "x-postit-multiline": true,
                "x-postit-label": m("credentials.postal_address.label", "Postal address"),
                "x-postit-help": m(
                    "credentials.postal_address.help",
                    "Printed in the footer of every campaign, as anti-spam laws require.",
                ),
            }),
        ),
    ]
}

/// The SMTP server, shown when the sending method is `smtp`.
fn smtp_properties(platform: &PlatformId) -> Vec<(&'static str, Value)> {
    let m = |key: &str, en: &str| message(platform, key, en);
    let smtp = json!({"field": "transport", "equals": "smtp"});
    vec![
        (
            "host",
            json!({
                "type": "string",
                "minLength": 1,
                "maxLength": 253,
                "x-postit-label": m("credentials.host.label", "SMTP host"),
                "x-postit-depends-on": smtp,
            }),
        ),
        (
            "port",
            json!({
                "type": "integer",
                "minimum": 1,
                "maximum": 65535,
                "x-postit-label": m("credentials.port.label", "Port"),
                "x-postit-help": m(
                    "credentials.port.help",
                    "Leave empty for the usual port: 587 with STARTTLS, 465 with TLS, 25 with none.",
                ),
                "x-postit-depends-on": smtp,
            }),
        ),
        (
            "tls",
            json!({
                "type": "string",
                "enum": ["starttls", "tls", "none"],
                "x-postit-enum-labels": [
                    m("credentials.tls.starttls", "STARTTLS"),
                    m("credentials.tls.tls", "TLS"),
                    m("credentials.tls.none", "None (local servers only)"),
                ],
                "default": "starttls",
                "x-postit-label": m("credentials.tls.label", "Encryption"),
                "x-postit-depends-on": smtp,
            }),
        ),
        (
            "username",
            json!({
                "type": "string",
                "maxLength": 254,
                "x-postit-label": m("credentials.username.label", "Username"),
                "x-postit-depends-on": smtp,
            }),
        ),
        (
            "password",
            json!({
                "type": "string",
                "maxLength": 500,
                "x-postit-secret": true,
                "x-postit-label": m("credentials.password.label", "Password"),
                "x-postit-depends-on": smtp,
            }),
        ),
    ]
}

/// The per-campaign options: whether opens are tracked.
#[must_use]
pub fn options_schema(platform: &PlatformId) -> OptionsSchema {
    let m = |key: &str, en: &str| message(platform, key, en);
    parse(
        platform,
        &json!({
            "type": "object",
            "properties": {
                "open_tracking": {
                    "type": "boolean",
                    "default": false,
                    "x-postit-label": m("options.open_tracking.label", "Track opens"),
                    "x-postit-help": m(
                        "options.open_tracking.help",
                        "Adds an invisible image that tells you when a contact opens the email.",
                    ),
                },
            },
            "additionalProperties": false,
        }),
    )
}

/// The email agent's descriptor.
#[must_use]
pub fn descriptor() -> AgentDescriptor {
    let id = platform_id();
    AgentDescriptor {
        connect: ConnectMethod::Credentials(credentials_schema(&id)),
        options_schema: options_schema(&id),
        platform: id,
        display_name: "Email".to_owned(),
        icon_png: ICON,
        brand_color: BRAND_COLOR.to_owned(),
        kind: AgentKind::Direct,
        // No attachments; image blocks embed stored PNG, JPEG, and GIF images.
        formats: vec![media_format(
            FormatId::HtmlEmail,
            text_rules(TextMeasure::Graphemes, MAX_TEXT, false),
            MediaRules {
                min_count: 0,
                max_count: 0,
                kinds: vec![MediaKind::Image],
                mime_types: vec![
                    "image/jpeg".to_owned(),
                    "image/png".to_owned(),
                    "image/gif".to_owned(),
                ],
                ..MediaRules::none()
            },
        )],
        capabilities: BTreeSet::from([Capability::DirectSender, Capability::SenderDomainCheck]),
        config_schema_version: 1,
        native_schedule: None,
        quota_buckets: Vec::new(),
    }
}
