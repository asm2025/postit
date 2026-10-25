//! Pins the JSON shape of the shared types. D3 and the clients depend on these shapes, so a
//! failing pin is a breaking API change, not a test to update casually.

#![allow(clippy::unwrap_used, clippy::panic)]

use std::collections::BTreeSet;

use postit_core::{
    AccountId, AccountRef, AgentDescriptor, AgentKind, Capability, ConnectMethod, DeliveryState,
    FormatId, FormatSpec, MediaRules, OptionsSchema, ParkReason, PlatformId, PostId, PostKind,
    PostVersion, PublishSnapshot, RecurrenceRule, TextMeasure, TextRules, Variant,
};
use serde_json::json;
use uuid::Uuid;

const NIL: &str = "00000000-0000-0000-0000-000000000000";

fn text_format() -> FormatSpec {
    FormatSpec {
        id: FormatId::Text,
        text: TextRules {
            measure: TextMeasure::Graphemes,
            max: 500,
            required: true,
        },
        media: MediaRules::none(),
        selected_by: None,
    }
}

fn text_format_json() -> serde_json::Value {
    json!({
        "id": "text",
        "text": {"measure": "graphemes", "max": 500, "required": true},
        "media": {
            "min_count": 0, "max_count": 0, "kinds": [], "mime_types": [],
            "max_bytes": null, "min_width": null, "max_width": null,
            "min_height": null, "max_height": null, "aspect_ratios": [],
            "min_duration_ms": null, "max_duration_ms": null, "safe_zones": []
        },
        "selected_by": null
    })
}

fn variant() -> Variant {
    Variant {
        target: AccountRef {
            platform: PlatformId::new("demo").unwrap(),
            account: AccountId::from_uuid(Uuid::nil()),
            remote_id: "remote".to_owned(),
        },
        kind: PostKind::Post,
        text: "hi".to_owned(),
        media: Vec::new(),
        link: None,
        tags: vec!["a".to_owned()],
        disclose_ai_generated: false,
        reply_to: None,
        email: None,
        options: serde_json::Map::new(),
    }
}

fn variant_json() -> serde_json::Value {
    json!({
        "target": {"platform": "demo", "account": NIL, "remote_id": "remote"},
        "kind": "post",
        "text": "hi",
        "media": [],
        "tags": ["a"],
        "disclose_ai_generated": false
    })
}

fn descriptor() -> AgentDescriptor {
    let schema = OptionsSchema::from_json_str(
        r#"{"type":"object","properties":{"token":{"type":"string","x-postit-secret":true,
            "x-postit-label":{"key":"demo.token","en":"Token"}}},
            "required":["token"],"additionalProperties":false}"#,
    )
    .unwrap();
    AgentDescriptor {
        platform: PlatformId::new("demo").unwrap(),
        display_name: "Demo".to_owned(),
        icon_png: &[0x89, b'P', b'N', b'G'],
        brand_color: "#112233".to_owned(),
        kind: AgentKind::Broadcast,
        connect: ConnectMethod::Credentials(schema),
        formats: vec![text_format()],
        capabilities: BTreeSet::from([Capability::Commenter]),
        config_schema_version: 1,
        native_schedule: None,
        quota_buckets: Vec::new(),
        options_schema: OptionsSchema::empty(),
    }
}

#[test]
fn format_spec_wire_format() {
    assert_eq!(
        serde_json::to_value(text_format()).unwrap(),
        text_format_json()
    );
}

#[test]
fn variant_wire_format() {
    assert_eq!(serde_json::to_value(variant()).unwrap(), variant_json());
    assert_eq!(
        serde_json::from_value::<Variant>(variant_json()).unwrap(),
        variant()
    );
}

#[test]
fn publish_snapshot_wire_format() {
    let snapshot = PublishSnapshot {
        post_id: PostId::from_uuid(Uuid::nil()),
        post_version: PostVersion(3),
        variants: vec![variant()],
    };
    let want = json!({"post_id": NIL, "post_version": 3, "variants": [variant_json()]});
    assert_eq!(serde_json::to_value(&snapshot).unwrap(), want);
    assert_eq!(
        serde_json::from_value::<PublishSnapshot>(want).unwrap(),
        snapshot
    );
}

#[test]
fn agent_descriptor_wire_format() {
    let want = json!({
        "platform": "demo",
        "display_name": "Demo",
        "brand_color": "#112233",
        "kind": "broadcast",
        "connect": {
            "method": "credentials",
            "schema": {
                "type": "object",
                "properties": {
                    "token": {
                        "type": "string",
                        "x-postit-label": {"key": "demo.token", "en": "Token"},
                        "x-postit-secret": true
                    }
                },
                "required": ["token"],
                "additionalProperties": false
            }
        },
        "formats": [text_format_json()],
        "capabilities": ["commenter"],
        "config_schema_version": 1,
        "native_schedule": null,
        "quota_buckets": [],
        "options_schema": {"type": "object", "properties": {}, "additionalProperties": false}
    });
    let got = serde_json::to_value(descriptor()).unwrap();
    assert_eq!(got, want);
    assert!(got.get("icon_png").is_none());
}

#[test]
fn recurrence_rule_wire_format() {
    let rule: RecurrenceRule = "FREQ=WEEKLY;BYDAY=MO".parse().unwrap();
    assert_eq!(
        serde_json::to_value(&rule).unwrap(),
        json!("FREQ=WEEKLY;BYDAY=MO")
    );
}

#[test]
fn parked_state_wire_format() {
    let state = DeliveryState::Parked(ParkReason::QuotaExceeded);
    let want = json!({"state": "parked", "reason": "quota_exceeded"});
    assert_eq!(serde_json::to_value(state).unwrap(), want);
    assert_eq!(
        serde_json::from_value::<DeliveryState>(want).unwrap(),
        state
    );
}

#[cfg(feature = "openapi")]
#[test]
fn schemas_match_wire_format() {
    use utoipa::openapi::RefOr;
    use utoipa::openapi::schema::{Schema, Type};
    use utoipa::{PartialSchema, ToSchema};

    let mut all: Vec<(String, RefOr<Schema>)> = Vec::new();
    AgentDescriptor::schemas(&mut all);
    all.push((
        AgentDescriptor::name().into_owned(),
        AgentDescriptor::schema(),
    ));
    let find = |name: &str| -> serde_json::Value {
        let (_, schema) = all
            .iter()
            .find(|(n, _)| n == name)
            .unwrap_or_else(|| panic!("schema {name} was not collected"));
        serde_json::to_value(schema).unwrap()
    };

    for name in ["AspectRatio", "PlatformId"] {
        assert_eq!(find(name)["type"], "string", "{name}");
    }
    assert_eq!(
        serde_json::to_value(RecurrenceRule::schema()).unwrap()["type"],
        "string"
    );
    assert!(matches!(
        RecurrenceRule::schema(),
        RefOr::T(Schema::Object(ref o)) if o.schema_type == utoipa::openapi::schema::SchemaType::Type(Type::String)
    ));

    let connect = find("ConnectMethod");
    assert!(
        connect["required"]
            .as_array()
            .is_some_and(|r| r.iter().any(|v| v == "method")),
        "ConnectMethod must require its `method` tag: {connect}"
    );
    let options = find("OptionsSchema");
    assert_eq!(options["type"], "object");
    assert!(
        options
            .get("properties")
            .is_none_or(|p| p.as_object().is_some_and(serde_json::Map::is_empty))
    );
    let descriptor = find("AgentDescriptor");
    assert!(descriptor["properties"].get("icon_png").is_none());
}
