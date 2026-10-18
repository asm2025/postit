//! The options and credentials schema subset.
//!
//! Agents describe their per-post options and connection credentials as JSON Schema, restricted
//! to a small subset that clients can render as forms. Anything outside the subset is rejected
//! when the schema is parsed, never silently dropped. `x-postit-*` keywords carry the hints a
//! form needs (labels, help, secrets, consent, dependencies).

mod credentials;
mod parse;
mod validate;

use serde::ser::{Serialize, SerializeMap, Serializer};
use serde_json::{Map, Value, json};

pub use credentials::{CredentialFields, CredentialValue};

use crate::Message;

/// Why a schema is outside the supported subset.
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
pub enum SchemaError {
    /// A JSON Schema keyword (or `x-postit-*` hint) the subset does not support.
    #[error("unsupported keyword `{keyword}` at `{path}`")]
    UnsupportedKeyword {
        /// Where in the schema (`$` for the root, `properties.<name>` for a property).
        path: String,
        /// The keyword.
        keyword: String,
    },
    /// A `type` the subset does not support.
    #[error("unsupported type `{ty}` at `{path}`")]
    UnsupportedType {
        /// Where in the schema.
        path: String,
        /// The type as written.
        ty: String,
    },
    /// The schema is malformed or combines supported keywords wrongly.
    #[error("invalid schema at `{path}`: {reason}")]
    Invalid {
        /// Where in the schema.
        path: String,
        /// What is wrong.
        reason: String,
    },
}

/// A parsed options (or credentials) schema. Field order is declaration order.
#[derive(Debug, Clone, PartialEq, Default)]
pub struct OptionsSchema {
    fields: Vec<SchemaField>,
}

/// One property of an [`OptionsSchema`].
#[derive(Debug, Clone, PartialEq)]
pub struct SchemaField {
    /// Property name; the key in the options object.
    pub name: String,
    /// Label shown next to the input.
    pub label: Message,
    /// Optional help text.
    pub help: Option<Message>,
    /// Listed in the schema's `required` array.
    pub required: bool,
    /// Type and constraints.
    pub kind: FieldKind,
    /// The user must choose a value: no default is allowed and the field counts as required.
    pub no_default: bool,
    /// The value is a secret (credentials); sealed in the vault, never echoed back.
    pub secret: bool,
    /// For a boolean: the consent text the user agrees to; the value must be `true`.
    pub consent_text: Option<Message>,
    /// The field applies only while another field has a given value.
    pub depends_on: Option<DependsOn>,
}

/// The type of a [`SchemaField`] and its constraints.
#[derive(Debug, Clone, PartialEq)]
pub enum FieldKind {
    /// A single-line string. Lengths count graphemes.
    String {
        /// Minimum length.
        min_length: Option<u32>,
        /// Maximum length.
        max_length: Option<u32>,
        /// Default value.
        default: Option<String>,
    },
    /// A multi-line string (`x-postit-multiline`).
    MultilineText {
        /// Maximum length in graphemes.
        max_length: Option<u32>,
        /// Default value.
        default: Option<String>,
    },
    /// One of a fixed set of strings.
    Enum {
        /// Allowed values with their labels.
        values: Vec<EnumValue>,
        /// Default value (one of `values`).
        default: Option<String>,
    },
    /// A boolean.
    Boolean {
        /// Default value.
        default: Option<bool>,
    },
    /// An integer.
    Integer {
        /// Inclusive minimum.
        minimum: Option<i64>,
        /// Inclusive maximum.
        maximum: Option<i64>,
        /// Default value.
        default: Option<i64>,
    },
    /// An RFC 3339 date-time string.
    DateTime,
}

/// One allowed value of an enum field.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct EnumValue {
    /// The wire value.
    pub value: String,
    /// Label shown to the user.
    pub label: Message,
}

/// Makes a field apply only while another, earlier field equals `equals`.
#[derive(Debug, Clone, PartialEq)]
pub struct DependsOn {
    /// Name of the earlier field.
    pub field: String,
    /// The value it must have.
    pub equals: Value,
}

impl OptionsSchema {
    /// A schema with no fields.
    #[must_use]
    pub fn empty() -> Self {
        Self::default()
    }

    /// The fields in declaration order.
    #[must_use]
    pub fn fields(&self) -> &[SchemaField] {
        &self.fields
    }

    fn field(&self, name: &str) -> Option<&SchemaField> {
        self.fields.iter().find(|f| f.name == name)
    }
}

impl SchemaField {
    /// The default as a JSON value, when the kind has one.
    pub(crate) fn default_value(&self) -> Option<Value> {
        match &self.kind {
            FieldKind::String { default, .. }
            | FieldKind::MultilineText { default, .. }
            | FieldKind::Enum { default, .. } => default.as_ref().map(|d| json!(d)),
            FieldKind::Boolean { default } => default.map(|d| json!(d)),
            FieldKind::Integer { default, .. } => default.map(|d| json!(d)),
            FieldKind::DateTime => None,
        }
    }

    fn to_json(&self) -> Value {
        let mut o = Map::new();
        o.insert("x-postit-label".into(), json!(self.label));
        if let Some(help) = &self.help {
            o.insert("x-postit-help".into(), json!(help));
        }
        if self.no_default {
            o.insert("x-postit-no-default".into(), json!(true));
        }
        if self.secret {
            o.insert("x-postit-secret".into(), json!(true));
        }
        if let Some(consent) = &self.consent_text {
            o.insert("x-postit-consent-text".into(), json!(consent));
        }
        if let Some(dep) = &self.depends_on {
            o.insert(
                "x-postit-depends-on".into(),
                json!({"field": dep.field, "equals": dep.equals}),
            );
        }
        match &self.kind {
            FieldKind::String {
                min_length,
                max_length,
                ..
            } => {
                o.insert("type".into(), json!("string"));
                insert_opt(&mut o, "minLength", *min_length);
                insert_opt(&mut o, "maxLength", *max_length);
            }
            FieldKind::MultilineText { max_length, .. } => {
                o.insert("type".into(), json!("string"));
                o.insert("x-postit-multiline".into(), json!(true));
                insert_opt(&mut o, "maxLength", *max_length);
            }
            FieldKind::Enum { values, .. } => {
                o.insert("type".into(), json!("string"));
                o.insert(
                    "enum".into(),
                    Value::Array(values.iter().map(|v| json!(v.value)).collect()),
                );
                o.insert(
                    "x-postit-enum-labels".into(),
                    Value::Array(values.iter().map(|v| json!(v.label)).collect()),
                );
            }
            FieldKind::Boolean { .. } => {
                o.insert("type".into(), json!("boolean"));
            }
            FieldKind::Integer {
                minimum, maximum, ..
            } => {
                o.insert("type".into(), json!("integer"));
                insert_opt(&mut o, "minimum", *minimum);
                insert_opt(&mut o, "maximum", *maximum);
            }
            FieldKind::DateTime => {
                o.insert("type".into(), json!("string"));
                o.insert("format".into(), json!("date-time"));
            }
        }
        if let Some(default) = self.default_value() {
            o.insert("default".into(), default);
        }
        Value::Object(o)
    }
}

fn insert_opt<T: Into<Value>>(o: &mut Map<String, Value>, key: &str, value: Option<T>) {
    if let Some(v) = value {
        o.insert(key.into(), v.into());
    }
}

struct Properties<'a>(&'a [SchemaField]);

impl Serialize for Properties<'_> {
    fn serialize<S: Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        let mut map = serializer.serialize_map(Some(self.0.len()))?;
        for field in self.0 {
            map.serialize_entry(&field.name, &field.to_json())?;
        }
        map.end()
    }
}

impl Serialize for OptionsSchema {
    /// Emits JSON Schema with `properties` in declaration order.
    fn serialize<S: Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        let required: Vec<&str> = self
            .fields
            .iter()
            .filter(|f| f.required)
            .map(|f| f.name.as_str())
            .collect();
        let mut map = serializer.serialize_map(None)?;
        map.serialize_entry("type", "object")?;
        map.serialize_entry("properties", &Properties(&self.fields))?;
        if !required.is_empty() {
            map.serialize_entry("required", &required)?;
        }
        map.serialize_entry("additionalProperties", &false)?;
        map.end()
    }
}

#[cfg(feature = "openapi")]
impl utoipa::PartialSchema for OptionsSchema {
    fn schema() -> utoipa::openapi::RefOr<utoipa::openapi::schema::Schema> {
        utoipa::openapi::ObjectBuilder::new()
            .schema_type(utoipa::openapi::schema::Type::Object)
            .description(Some(
                "A JSON Schema document in the postit options subset (free-form object).",
            ))
            .into()
    }
}

#[cfg(feature = "openapi")]
impl utoipa::ToSchema for OptionsSchema {}

#[cfg(test)]
#[allow(clippy::unwrap_used, clippy::panic)]
mod tests {
    use secrecy::ExposeSecret;

    use super::*;
    use crate::ViolationCode;

    const LBL: &str = r#"{"key":"k","en":"E"}"#;

    fn schema(src: &str) -> OptionsSchema {
        OptionsSchema::from_json_str(&src.replace("LBL", LBL)).unwrap()
    }

    fn codes(s: &OptionsSchema, values: Value) -> Vec<ViolationCode> {
        let Value::Object(map) = values else {
            unreachable!()
        };
        s.validate(&map).iter().map(|v| v.code).collect()
    }

    const FULL: &str = r#"{
        "type":"object",
        "properties":{
            "title":{"type":"string","minLength":1,"maxLength":10,"x-postit-label":LBL},
            "caption":{"type":"string","x-postit-multiline":true,"maxLength":100,"x-postit-label":LBL},
            "privacy":{"type":"string","enum":["public","private"],"default":"public",
                "x-postit-enum-labels":[LBL,LBL],"x-postit-label":LBL,"x-postit-help":LBL},
            "allow_comments":{"type":"boolean","default":true,"x-postit-label":LBL},
            "rating":{"type":"integer","minimum":1,"maximum":5,"default":3,"x-postit-label":LBL},
            "publish_at":{"type":"string","format":"date-time","x-postit-label":LBL}
        },
        "required":["title"],
        "additionalProperties":false
    }"#;

    #[test]
    fn parses_every_field_kind() {
        let s = schema(FULL);
        let names: Vec<&str> = s.fields().iter().map(|f| f.name.as_str()).collect();
        assert_eq!(
            names,
            [
                "title",
                "caption",
                "privacy",
                "allow_comments",
                "rating",
                "publish_at"
            ]
        );
        let f = s.fields();
        assert_eq!(
            f[0].kind,
            FieldKind::String {
                min_length: Some(1),
                max_length: Some(10),
                default: None
            }
        );
        assert!(f[0].required);
        assert_eq!(
            f[1].kind,
            FieldKind::MultilineText {
                max_length: Some(100),
                default: None
            }
        );
        let FieldKind::Enum { values, default } = &f[2].kind else {
            panic!("enum expected")
        };
        assert_eq!(values.len(), 2);
        assert_eq!(values[1].value, "private");
        assert_eq!(default.as_deref(), Some("public"));
        assert!(f[2].help.is_some());
        assert_eq!(
            f[3].kind,
            FieldKind::Boolean {
                default: Some(true)
            }
        );
        assert_eq!(
            f[4].kind,
            FieldKind::Integer {
                minimum: Some(1),
                maximum: Some(5),
                default: Some(3)
            }
        );
        assert_eq!(f[5].kind, FieldKind::DateTime);
    }

    enum Expect {
        Keyword(&'static str),
        Type(&'static str),
        Invalid(&'static str),
    }

    fn one(body: &str) -> String {
        format!(r#"{{"type":"object","properties":{{"f":{body}}}}}"#).replace("LBL", LBL)
    }

    #[test]
    fn rejects_out_of_subset() {
        use Expect::{Invalid, Keyword, Type};
        let cases: Vec<(String, Expect)> = vec![
            (one(r#"{"oneOf":[{"type":"string"}],"x-postit-label":LBL}"#), Keyword("oneOf")),
            (one(r#"{"type":"string","pattern":"^a","x-postit-label":LBL}"#), Keyword("pattern")),
            (one(r##"{"$ref":"#/defs/x"}"##), Keyword("$ref")),
            (one(r#"{"type":"string","x-postit-unknown":1,"x-postit-label":LBL}"#), Keyword("x-postit-unknown")),
            (r#"{"type":"object","oneOf":[],"properties":{}}"#.into(), Keyword("oneOf")),
            (one(r#"{"type":"array","x-postit-label":LBL}"#), Type("array")),
            (one(r#"{"type":"number","x-postit-label":LBL}"#), Type("number")),
            (r#"{"type":"array","properties":{}}"#.into(), Type("array")),
            (one(r#"{"type":"string","format":"email","x-postit-label":LBL}"#), Invalid("properties.f")),
            (r#"{"type":"object","properties":{},"additionalProperties":true}"#.into(), Invalid("$.additionalProperties")),
            (one(r#"{"type":"string","enum":["a"],"x-postit-label":LBL}"#), Invalid("properties.f")),
            (one(r#"{"type":"string","default":"x","x-postit-no-default":true,"x-postit-label":LBL}"#), Invalid("properties.f")),
            (one(r#"{"type":"string","x-postit-consent-text":LBL,"x-postit-label":LBL}"#), Invalid("properties.f")),
            (one(r#"{"type":"string","x-postit-label":LBL,"x-postit-depends-on":{"field":"ghost","equals":1}}"#), Invalid("properties.f")),
            (
                r#"{"type":"object","properties":{
                    "a":{"type":"string","x-postit-label":LBL,"x-postit-depends-on":{"field":"b","equals":true}},
                    "b":{"type":"boolean","x-postit-label":LBL}}}"#
                    .replace("LBL", LBL),
                Invalid("properties.a"),
            ),
            (one(r#"{"type":"string","x-postit-label":LBL,"minimum":1}"#), Invalid("properties.f")),
            (one(r#"{"type":"string"}"#), Invalid("properties.f")),
        ];
        for (src, expect) in cases {
            let err = OptionsSchema::from_json_str(&src).unwrap_err();
            match (expect, &err) {
                (Expect::Keyword(k), SchemaError::UnsupportedKeyword { keyword, .. }) => {
                    assert_eq!(keyword, k, "{src}");
                }
                (Expect::Type(t), SchemaError::UnsupportedType { ty, .. }) => {
                    assert_eq!(ty, t, "{src}");
                }
                (Expect::Invalid(p), SchemaError::Invalid { path, .. }) => {
                    assert_eq!(path, p, "{src}");
                }
                _ => panic!("wrong error {err:?} for {src}"),
            }
        }
    }

    #[test]
    fn keyword_errors_name_the_property() {
        let err = OptionsSchema::from_json_str(&one(
            r#"{"type":"string","pattern":"x","x-postit-label":LBL}"#,
        ))
        .unwrap_err();
        assert_eq!(
            err,
            SchemaError::UnsupportedKeyword {
                path: "properties.f".into(),
                keyword: "pattern".into()
            }
        );
    }

    #[test]
    fn serializes_back_to_json_schema() {
        let input = FULL.replace("LBL", LBL);
        let s = schema(FULL);
        let out = serde_json::to_string(&s).unwrap();
        let pos = |name: &str| out.find(&format!("\"{name}\":{{")).unwrap();
        assert!(pos("title") < pos("caption"));
        assert!(pos("caption") < pos("privacy"));
        assert!(pos("privacy") < pos("allow_comments"));
        assert!(pos("allow_comments") < pos("rating"));
        assert!(pos("rating") < pos("publish_at"));
        assert_eq!(
            serde_json::from_str::<Value>(&out).unwrap(),
            serde_json::from_str::<Value>(&input).unwrap()
        );
        assert_eq!(OptionsSchema::from_json_str(&out).unwrap(), s);
    }

    #[test]
    fn empty_schema_serializes() {
        assert_eq!(
            serde_json::to_value(OptionsSchema::empty()).unwrap(),
            json!({"type":"object","properties":{},"additionalProperties":false})
        );
    }

    #[test]
    fn validate_reports_all_violations() {
        let s = schema(
            r#"{"type":"object","properties":{
                "must":{"type":"string","x-postit-label":LBL},
                "title":{"type":"string","maxLength":5,"x-postit-label":LBL},
                "mode":{"type":"string","enum":["a","b"],"x-postit-enum-labels":[LBL,LBL],"x-postit-label":LBL},
                "count":{"type":"integer","minimum":1,"maximum":3,"x-postit-label":LBL}
            },"required":["must"]}"#,
        );
        let got = codes(
            &s,
            json!({"zzz":1,"mode":"c","count":"x","title":"too long text"}),
        );
        assert_eq!(
            got,
            [
                ViolationCode::OptionRequired,
                ViolationCode::OptionTooLong,
                ViolationCode::OptionNotAllowed,
                ViolationCode::OptionType,
                ViolationCode::OptionUnknown,
            ]
        );
        let Value::Object(m) = json!({"must":"x","count":9}) else {
            unreachable!()
        };
        let v = s.validate(&m);
        assert_eq!(v.len(), 1);
        assert_eq!(v[0].code, ViolationCode::OptionOutOfRange);
        assert_eq!(v[0].path.as_deref(), Some("options.count"));
        assert_eq!(v[0].params["min"], json!(1));
        assert_eq!(v[0].params["max"], json!(3));
        assert_eq!(v[0].params["name"], json!("count"));
    }

    #[test]
    fn string_length_counts_graphemes() {
        let s = schema(
            r#"{"type":"object","properties":{
                "t":{"type":"string","minLength":2,"maxLength":3,"x-postit-label":LBL}}}"#,
        );
        // Family emoji ZWJ sequence = 1 grapheme.
        assert_eq!(
            codes(
                &s,
                json!({"t": "\u{1F468}\u{200D}\u{1F469}\u{200D}\u{1F467}"})
            ),
            [ViolationCode::OptionTooShort]
        );
        assert!(codes(&s, json!({"t": "abc"})).is_empty());
        assert_eq!(
            codes(&s, json!({"t": "abcd"})),
            [ViolationCode::OptionTooLong]
        );
    }

    #[test]
    fn depends_on_skips_hidden_fields() {
        let s = schema(
            r#"{"type":"object","properties":{
                "kind":{"type":"string","enum":["a","b"],"x-postit-enum-labels":[LBL,LBL],"x-postit-label":LBL},
                "extra":{"type":"string","x-postit-label":LBL,
                    "x-postit-depends-on":{"field":"kind","equals":"b"}}
            },"required":["extra"]}"#,
        );
        assert!(codes(&s, json!({"kind":"a"})).is_empty());
        assert!(codes(&s, json!({"kind":"a","extra":5})).is_empty());
        assert_eq!(
            codes(&s, json!({"kind":"b"})),
            [ViolationCode::OptionRequired]
        );
        assert_eq!(
            codes(&s, json!({"kind":"b","extra":5})),
            [ViolationCode::OptionType]
        );
    }

    #[test]
    fn no_default_field_is_required() {
        let s = schema(
            r#"{"type":"object","properties":{
                "privacy":{"type":"string","enum":["a","b"],"x-postit-enum-labels":[LBL,LBL],
                    "x-postit-no-default":true,"x-postit-label":LBL}}}"#,
        );
        assert_eq!(codes(&s, json!({})), [ViolationCode::OptionRequired]);
        assert!(codes(&s, json!({"privacy":"a"})).is_empty());
    }

    #[test]
    fn consent_must_be_true() {
        let s = schema(
            r#"{"type":"object","properties":{
                "agree":{"type":"boolean","x-postit-consent-text":LBL,"x-postit-label":LBL}}}"#,
        );
        assert_eq!(
            codes(&s, json!({"agree":false})),
            [ViolationCode::OptionConsentRequired]
        );
        assert!(codes(&s, json!({"agree":true})).is_empty());
        assert_eq!(
            codes(&s, json!({"agree":"yes"})),
            [ViolationCode::OptionType]
        );
    }

    #[test]
    fn date_time_must_be_rfc3339() {
        let s = schema(
            r#"{"type":"object","properties":{
                "at":{"type":"string","format":"date-time","x-postit-label":LBL}}}"#,
        );
        assert!(codes(&s, json!({"at":"2026-10-18T10:00:00Z"})).is_empty());
        assert!(codes(&s, json!({"at":"2026-10-18T10:00:00+03:00"})).is_empty());
        assert_eq!(
            codes(&s, json!({"at":"yesterday"})),
            [ViolationCode::OptionType]
        );
        assert_eq!(
            codes(&s, json!({"at":"2026-10-18"})),
            [ViolationCode::OptionType]
        );
        assert_eq!(codes(&s, json!({"at":5})), [ViolationCode::OptionType]);
    }

    #[test]
    fn credentials_split_secrets() {
        let s = schema(
            r#"{"type":"object","properties":{
                "bot_token":{"type":"string","x-postit-secret":true,"x-postit-label":LBL},
                "chat":{"type":"string","x-postit-label":LBL}
            },"required":["bot_token","chat"]}"#,
        );
        let Value::Object(values) = json!({"bot_token":"tok-secret-value","chat":"@room"}) else {
            unreachable!()
        };
        let creds = s.parse_credentials(values).unwrap();
        assert_eq!(
            creds.get_secret("bot_token").unwrap().expose_secret(),
            "tok-secret-value"
        );
        assert_eq!(creds.get_plain("chat"), Some(&json!("@room")));
        assert!(creds.get_plain("bot_token").is_none());
        assert!(creds.get_secret("chat").is_none());
        let debug = format!("{creds:?}");
        assert!(!debug.contains("tok-secret-value"), "{debug}");
        assert!(debug.contains("[redacted]"));
    }

    #[test]
    fn credentials_report_violations() {
        let s = schema(
            r#"{"type":"object","properties":{
                "bot_token":{"type":"string","x-postit-secret":true,"x-postit-label":LBL}
            },"required":["bot_token"]}"#,
        );
        let err = s.parse_credentials(Map::new()).unwrap_err();
        assert_eq!(err[0].code, ViolationCode::OptionRequired);
    }
}
