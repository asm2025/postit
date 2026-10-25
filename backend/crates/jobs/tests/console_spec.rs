use postit_jobs::ConsoleSpec;
use serde_json::json;

const KINDS: &[&str] = &["user_pending_approval", "user_approved"];

#[test]
fn a_summary_keeps_only_declared_ids_and_enum_values() {
    let spec = ConsoleSpec::new()
        .id_field("recipient", "recipient_user_id")
        .enum_field("kind", "mail_kind", KINDS);
    let payload = json!({
        "recipient": "0199c3d8-0000-7000-8000-000000000001",
        "kind": "user_approved",
        "body": "free text that must never appear",
    });
    let summary = spec.summarize(&payload);
    assert_eq!(
        summary.0.get("recipient_user_id").map(String::as_str),
        Some("0199c3d8-0000-7000-8000-000000000001")
    );
    assert_eq!(
        summary.0.get("mail_kind").map(String::as_str),
        Some("user_approved")
    );
    assert_eq!(summary.0.len(), 2);
}

#[test]
fn values_that_are_not_ids_or_plain_tokens_are_dropped() {
    let spec = ConsoleSpec::new()
        .id_field("recipient", "recipient_user_id")
        .enum_field("kind", "mail_kind", KINDS);
    let payload = json!({ "recipient": "ada@example.com", "kind": "Hello, Ada <ada@example.com>" });
    assert!(spec.summarize(&payload).0.is_empty());
}

#[test]
fn an_enum_value_outside_the_declared_set_is_dropped() {
    let spec = ConsoleSpec::new().enum_field("kind", "mail_kind", KINDS);
    let payload = json!({ "kind": "secret_word" });
    assert!(spec.summarize(&payload).0.is_empty());
}

#[test]
#[should_panic(expected = "must end in `_id`")]
fn an_id_label_must_end_in_id() {
    let _ = ConsoleSpec::new().id_field("recipient", "recipient");
}
