//! Validating option values against an [`OptionsSchema`].

use serde_json::{Map, Value};
use unicode_segmentation::UnicodeSegmentation;

use super::{FieldKind, OptionsSchema, SchemaField};
use crate::{Violation, ViolationCode};

impl OptionsSchema {
    /// Checks `values` against the schema and returns every violation, in field order with
    /// unknown keys last. An empty vector means the values are valid.
    #[must_use]
    pub fn validate(&self, values: &Map<String, Value>) -> Vec<Violation> {
        let mut out = Vec::new();
        for field in &self.fields {
            if !self.applies(field, values) {
                continue;
            }
            match values.get(&field.name) {
                None => {
                    if field.required || field.no_default {
                        out.push(violation(ViolationCode::OptionRequired, field));
                    }
                }
                Some(value) => check_value(field, value, &mut out),
            }
        }
        for key in values.keys() {
            if self.field(key).is_none() {
                out.push(
                    Violation::new(ViolationCode::OptionUnknown)
                        .at(format!("options.{key}"))
                        .param("name", key.as_str()),
                );
            }
        }
        out
    }

    /// Whether the field is shown, given its `depends_on`. A dependency on a field the caller
    /// left out falls back to that field's default.
    pub(super) fn applies(&self, field: &SchemaField, values: &Map<String, Value>) -> bool {
        let Some(dep) = &field.depends_on else {
            return true;
        };
        let current = values
            .get(&dep.field)
            .cloned()
            .or_else(|| self.field(&dep.field).and_then(SchemaField::default_value));
        current.as_ref() == Some(&dep.equals)
    }
}

fn violation(code: ViolationCode, field: &SchemaField) -> Violation {
    Violation::new(code)
        .at(format!("options.{}", field.name))
        .param("name", field.name.as_str())
}

fn wrong_type(field: &SchemaField, expected: &str) -> Violation {
    violation(ViolationCode::OptionType, field).param("expected", expected)
}

fn grapheme_len(s: &str) -> u64 {
    s.graphemes(true).count() as u64
}

fn check_value(field: &SchemaField, value: &Value, out: &mut Vec<Violation>) {
    match &field.kind {
        FieldKind::String {
            min_length,
            max_length,
            ..
        } => check_text(field, value, *min_length, *max_length, out),
        FieldKind::MultilineText { max_length, .. } => {
            check_text(field, value, None, *max_length, out);
        }
        FieldKind::Enum { values, .. } => match value.as_str() {
            None => out.push(wrong_type(field, "string")),
            Some(s) if values.iter().any(|v| v.value == s) => {}
            Some(_) => out.push(violation(ViolationCode::OptionNotAllowed, field).param(
                "allowed",
                Value::from(values.iter().map(|v| v.value.as_str()).collect::<Vec<_>>()),
            )),
        },
        FieldKind::Boolean { .. } => match value.as_bool() {
            None => out.push(wrong_type(field, "boolean")),
            Some(b) => {
                if field.consent_text.is_some() && !b {
                    out.push(violation(ViolationCode::OptionConsentRequired, field));
                }
            }
        },
        FieldKind::Integer {
            minimum, maximum, ..
        } => match value.as_i64() {
            None => out.push(wrong_type(field, "integer")),
            Some(n) => {
                if minimum.is_some_and(|m| n < m) || maximum.is_some_and(|m| n > m) {
                    let mut v = violation(ViolationCode::OptionOutOfRange, field);
                    if let Some(m) = minimum {
                        v = v.param("min", *m);
                    }
                    if let Some(m) = maximum {
                        v = v.param("max", *m);
                    }
                    out.push(v);
                }
            }
        },
        FieldKind::DateTime => match value.as_str() {
            Some(s) if chrono::DateTime::parse_from_rfc3339(s).is_ok() => {}
            _ => out.push(wrong_type(field, "date-time")),
        },
    }
}

fn check_text(
    field: &SchemaField,
    value: &Value,
    min: Option<u32>,
    max: Option<u32>,
    out: &mut Vec<Violation>,
) {
    let Some(text) = value.as_str() else {
        out.push(wrong_type(field, "string"));
        return;
    };
    let len = grapheme_len(text);
    if let Some(min) = min
        && len < u64::from(min)
    {
        out.push(violation(ViolationCode::OptionTooShort, field).param("min", min));
    }
    if let Some(max) = max
        && len > u64::from(max)
    {
        out.push(violation(ViolationCode::OptionTooLong, field).param("max", max));
    }
}
