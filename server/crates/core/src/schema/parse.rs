//! Parsing JSON Schema text into the supported subset.

use std::fmt;

use serde::Deserialize;
use serde::de::{self, Deserializer, MapAccess, Visitor};
use serde_json::{Map, Value};

use super::{DependsOn, EnumValue, FieldKind, OptionsSchema, SchemaError, SchemaField};
use crate::Message;

/// Property keywords the subset knows. Anything else is `UnsupportedKeyword`.
const PROPERTY_KEYWORDS: &[&str] = &[
    "type",
    "format",
    "enum",
    "default",
    "minLength",
    "maxLength",
    "minimum",
    "maximum",
    "x-postit-label",
    "x-postit-help",
    "x-postit-enum-labels",
    "x-postit-multiline",
    "x-postit-no-default",
    "x-postit-secret",
    "x-postit-consent-text",
    "x-postit-depends-on",
];

/// A JSON object read as ordered pairs, so declaration order survives without `serde_json`'s
/// `preserve_order` feature (which would change key order workspace-wide).
struct Ordered(Vec<(String, Value)>);

impl<'de> Deserialize<'de> for Ordered {
    fn deserialize<D: Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
        struct V;
        impl<'de> Visitor<'de> for V {
            type Value = Ordered;

            fn expecting(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
                f.write_str("an object")
            }

            fn visit_map<A: MapAccess<'de>>(self, mut map: A) -> Result<Ordered, A::Error> {
                let mut pairs: Vec<(String, Value)> = Vec::new();
                while let Some((k, v)) = map.next_entry::<String, Value>()? {
                    if pairs.iter().any(|(existing, _)| *existing == k) {
                        return Err(de::Error::custom(format!("duplicate property `{k}`")));
                    }
                    pairs.push((k, v));
                }
                Ok(Ordered(pairs))
            }
        }
        deserializer.deserialize_map(V)
    }
}

/// The schema root: `properties` in order, every other key as a plain value.
struct Root {
    properties: Option<Vec<(String, Value)>>,
    rest: Map<String, Value>,
}

impl<'de> Deserialize<'de> for Root {
    fn deserialize<D: Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
        struct V;
        impl<'de> Visitor<'de> for V {
            type Value = Root;

            fn expecting(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
                f.write_str("a JSON Schema object")
            }

            fn visit_map<A: MapAccess<'de>>(self, mut map: A) -> Result<Root, A::Error> {
                let mut properties = None;
                let mut rest = Map::new();
                while let Some(key) = map.next_key::<String>()? {
                    if key == "properties" {
                        properties = Some(map.next_value::<Ordered>()?.0);
                    } else {
                        rest.insert(key, map.next_value::<Value>()?);
                    }
                }
                Ok(Root { properties, rest })
            }
        }
        deserializer.deserialize_map(V)
    }
}

fn invalid(path: &str, reason: impl Into<String>) -> SchemaError {
    SchemaError::Invalid {
        path: path.to_owned(),
        reason: reason.into(),
    }
}

impl OptionsSchema {
    /// Parses a schema, rejecting everything outside the supported subset.
    ///
    /// # Errors
    /// Returns a [`SchemaError`] naming the first unsupported keyword or type, or the first
    /// malformed or contradictory property.
    pub fn from_json_str(src: &str) -> Result<Self, SchemaError> {
        let root: Root = serde_json::from_str(src).map_err(|e| invalid("$", e.to_string()))?;
        build(root)
    }
}

fn build(root: Root) -> Result<OptionsSchema, SchemaError> {
    let mut rest = root.rest;
    match rest.remove("type") {
        Some(Value::String(t)) if t == "object" => {}
        Some(other) => {
            return Err(SchemaError::UnsupportedType {
                path: "$".into(),
                ty: type_text(&other),
            });
        }
        None => return Err(invalid("$", "missing `type`")),
    }
    let properties = root
        .properties
        .ok_or_else(|| invalid("$", "missing `properties`"))?;
    let required = match rest.remove("required") {
        None => Vec::new(),
        Some(Value::Array(items)) => items
            .into_iter()
            .map(|v| match v {
                Value::String(s) => Ok(s),
                _ => Err(invalid("$.required", "entries must be strings")),
            })
            .collect::<Result<Vec<_>, _>>()?,
        Some(_) => return Err(invalid("$.required", "must be an array")),
    };
    match rest.remove("additionalProperties") {
        None | Some(Value::Bool(false)) => {}
        Some(_) => {
            return Err(invalid(
                "$.additionalProperties",
                "only `false` is supported",
            ));
        }
    }
    if let Some(keyword) = rest.keys().next() {
        return Err(SchemaError::UnsupportedKeyword {
            path: "$".into(),
            keyword: keyword.clone(),
        });
    }
    if let Some(ghost) = required
        .iter()
        .find(|r| !properties.iter().any(|(n, _)| n == *r))
    {
        return Err(invalid(
            "$.required",
            format!("`{ghost}` is not a property"),
        ));
    }
    let mut fields: Vec<SchemaField> = Vec::with_capacity(properties.len());
    for (name, value) in properties {
        let is_required = required.contains(&name);
        let field = parse_field(name, value, is_required, &fields)?;
        fields.push(field);
    }
    Ok(OptionsSchema { fields })
}

fn type_text(v: &Value) -> String {
    match v {
        Value::String(s) => s.clone(),
        other => other.to_string(),
    }
}

fn take_bool(obj: &mut Map<String, Value>, key: &str, path: &str) -> Result<bool, SchemaError> {
    match obj.remove(key) {
        None => Ok(false),
        Some(Value::Bool(b)) => Ok(b),
        Some(_) => Err(invalid(path, format!("`{key}` must be a boolean"))),
    }
}

fn take_u32(
    obj: &mut Map<String, Value>,
    key: &str,
    path: &str,
) -> Result<Option<u32>, SchemaError> {
    match obj.remove(key) {
        None => Ok(None),
        Some(v) => v
            .as_u64()
            .and_then(|n| u32::try_from(n).ok())
            .map(Some)
            .ok_or_else(|| invalid(path, format!("`{key}` must be a non-negative integer"))),
    }
}

fn take_i64(
    obj: &mut Map<String, Value>,
    key: &str,
    path: &str,
) -> Result<Option<i64>, SchemaError> {
    match obj.remove(key) {
        None => Ok(None),
        Some(v) => v
            .as_i64()
            .map(Some)
            .ok_or_else(|| invalid(path, format!("`{key}` must be an integer"))),
    }
}

fn message(v: Value, key: &str, path: &str) -> Result<Message, SchemaError> {
    serde_json::from_value(v).map_err(|_| {
        invalid(
            path,
            format!("`{key}` must be an object with `key` and `en`"),
        )
    })
}

fn take_message(
    obj: &mut Map<String, Value>,
    key: &str,
    path: &str,
) -> Result<Option<Message>, SchemaError> {
    obj.remove(key).map(|v| message(v, key, path)).transpose()
}

fn parse_field(
    name: String,
    value: Value,
    required: bool,
    earlier: &[SchemaField],
) -> Result<SchemaField, SchemaError> {
    let path = format!("properties.{name}");
    let path = path.as_str();
    let Value::Object(mut obj) = value else {
        return Err(invalid(path, "a property must be an object"));
    };
    if let Some(keyword) = obj
        .keys()
        .find(|k| !PROPERTY_KEYWORDS.contains(&k.as_str()))
    {
        return Err(SchemaError::UnsupportedKeyword {
            path: path.to_owned(),
            keyword: keyword.clone(),
        });
    }
    let ty = match obj.remove("type") {
        Some(Value::String(s)) => s,
        Some(other) => {
            return Err(SchemaError::UnsupportedType {
                path: path.to_owned(),
                ty: other.to_string(),
            });
        }
        None => return Err(invalid(path, "missing `type`")),
    };
    if !matches!(ty.as_str(), "string" | "boolean" | "integer") {
        return Err(SchemaError::UnsupportedType {
            path: path.to_owned(),
            ty,
        });
    }

    let label = take_message(&mut obj, "x-postit-label", path)?
        .ok_or_else(|| invalid(path, "missing `x-postit-label`"))?;
    let help = take_message(&mut obj, "x-postit-help", path)?;
    let no_default = take_bool(&mut obj, "x-postit-no-default", path)?;
    let secret = take_bool(&mut obj, "x-postit-secret", path)?;
    let depends_on = match obj.remove("x-postit-depends-on") {
        None => None,
        Some(v) => Some(parse_depends_on(v, path, earlier)?),
    };
    let default = obj.remove("default");
    if no_default && default.is_some() {
        return Err(invalid(path, "`x-postit-no-default` forbids `default`"));
    }

    let mut consent_text = None;
    let kind = match ty.as_str() {
        "string" => string_kind(&mut obj, path, default)?,
        "boolean" => {
            consent_text = take_message(&mut obj, "x-postit-consent-text", path)?;
            let default = match default {
                None => None,
                Some(Value::Bool(b)) => Some(b),
                Some(_) => return Err(invalid(path, "`default` must be a boolean")),
            };
            FieldKind::Boolean { default }
        }
        _ => {
            let minimum = take_i64(&mut obj, "minimum", path)?;
            let maximum = take_i64(&mut obj, "maximum", path)?;
            if minimum > maximum && minimum.is_some() && maximum.is_some() {
                return Err(invalid(path, "`minimum` is greater than `maximum`"));
            }
            let default = match default {
                None => None,
                Some(v) => Some(
                    v.as_i64()
                        .ok_or_else(|| invalid(path, "`default` must be an integer"))?,
                ),
            };
            FieldKind::Integer {
                minimum,
                maximum,
                default,
            }
        }
    };
    if let Some(keyword) = obj.keys().next() {
        return Err(invalid(
            path,
            format!("`{keyword}` does not apply to a {ty} property"),
        ));
    }
    Ok(SchemaField {
        name,
        label,
        help,
        required,
        kind,
        no_default,
        secret,
        consent_text,
        depends_on,
    })
}

fn string_kind(
    obj: &mut Map<String, Value>,
    path: &str,
    default: Option<Value>,
) -> Result<FieldKind, SchemaError> {
    let format = obj.remove("format");
    let enum_values = obj.remove("enum");
    let enum_labels = obj.remove("x-postit-enum-labels");
    let multiline = take_bool(obj, "x-postit-multiline", path)?;
    let min_length = take_u32(obj, "minLength", path)?;
    let max_length = take_u32(obj, "maxLength", path)?;
    if let (Some(min), Some(max)) = (min_length, max_length)
        && min > max
    {
        return Err(invalid(path, "`minLength` is greater than `maxLength`"));
    }
    let default = match default {
        None => None,
        Some(Value::String(s)) => Some(s),
        Some(_) => return Err(invalid(path, "`default` must be a string")),
    };

    if let Some(format) = format {
        if format != "date-time" {
            return Err(invalid(path, "`format` must be `date-time`"));
        }
        if enum_values.is_some()
            || enum_labels.is_some()
            || multiline
            || min_length.is_some()
            || max_length.is_some()
            || default.is_some()
        {
            return Err(invalid(
                path,
                "a date-time takes no enum, multiline, length, or default",
            ));
        }
        return Ok(FieldKind::DateTime);
    }
    if let Some(values) = enum_values {
        if multiline || min_length.is_some() || max_length.is_some() {
            return Err(invalid(
                path,
                "an enum takes no multiline or length keywords",
            ));
        }
        return enum_kind(values, enum_labels, default, path);
    }
    if enum_labels.is_some() {
        return Err(invalid(path, "`x-postit-enum-labels` needs `enum`"));
    }
    if multiline {
        if min_length.is_some() {
            return Err(invalid(path, "a multiline text takes no `minLength`"));
        }
        return Ok(FieldKind::MultilineText {
            max_length,
            default,
        });
    }
    Ok(FieldKind::String {
        min_length,
        max_length,
        default,
    })
}

fn enum_kind(
    values: Value,
    labels: Option<Value>,
    default: Option<String>,
    path: &str,
) -> Result<FieldKind, SchemaError> {
    let Value::Array(values) = values else {
        return Err(invalid(path, "`enum` must be an array"));
    };
    let values: Vec<String> = values
        .into_iter()
        .map(|v| match v {
            Value::String(s) => Ok(s),
            _ => Err(invalid(path, "`enum` entries must be strings")),
        })
        .collect::<Result<_, _>>()?;
    if values.is_empty() {
        return Err(invalid(path, "`enum` must not be empty"));
    }
    let Some(Value::Array(labels)) = labels else {
        return Err(invalid(
            path,
            "`enum` needs `x-postit-enum-labels` (an array)",
        ));
    };
    if labels.len() != values.len() {
        return Err(invalid(
            path,
            "`x-postit-enum-labels` must be as long as `enum`",
        ));
    }
    if let Some(d) = &default
        && !values.contains(d)
    {
        return Err(invalid(path, "`default` is not one of `enum`"));
    }
    let values = values
        .into_iter()
        .zip(labels)
        .map(|(value, label)| {
            Ok(EnumValue {
                value,
                label: message(label, "x-postit-enum-labels", path)?,
            })
        })
        .collect::<Result<Vec<_>, SchemaError>>()?;
    Ok(FieldKind::Enum { values, default })
}

fn parse_depends_on(
    value: Value,
    path: &str,
    earlier: &[SchemaField],
) -> Result<DependsOn, SchemaError> {
    let Value::Object(mut o) = value else {
        return Err(invalid(
            path,
            "`x-postit-depends-on` must be an object with `field` and `equals`",
        ));
    };
    let Some(Value::String(field)) = o.remove("field") else {
        return Err(invalid(
            path,
            "`x-postit-depends-on.field` must be a string",
        ));
    };
    let equals = o
        .remove("equals")
        .ok_or_else(|| invalid(path, "`x-postit-depends-on.equals` is missing"))?;
    if let Some(extra) = o.keys().next() {
        return Err(invalid(
            path,
            format!("`x-postit-depends-on.{extra}` is not supported"),
        ));
    }
    if !earlier.iter().any(|f| f.name == field) {
        return Err(invalid(
            path,
            format!("`x-postit-depends-on` names `{field}`, which is not an earlier property"),
        ));
    }
    Ok(DependsOn { field, equals })
}
