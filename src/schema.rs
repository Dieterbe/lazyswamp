use std::collections::BTreeSet;

use regex::Regex;
use serde_json::{Map, Value};

use crate::error::{Error, Result};

#[derive(Debug, Clone, PartialEq)]
pub enum FormMode {
    Fields(Vec<FormField>),
    RawJson(String),
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum FieldKind {
    String,
    Integer,
    Number,
    Boolean,
    Enum(Vec<String>),
    Json,
}

#[derive(Debug, Clone, PartialEq)]
pub struct FormField {
    pub path: Vec<String>,
    pub label: String,
    pub description: String,
    pub required: bool,
    pub secret: bool,
    pub kind: FieldKind,
    pub value: String,
}

impl FormField {
    pub fn display_value(&self) -> String {
        if self.secret && !self.value.is_empty() {
            "••••••••".to_owned()
        } else {
            self.value.clone()
        }
    }
}

pub fn form_for(schema: &Value) -> FormMode {
    if contains_unsupported(schema) {
        return FormMode::RawJson("{}".to_owned());
    }
    let mut fields = Vec::new();
    collect_fields(schema, &[], false, &mut fields);
    if fields.is_empty() && schema.get("type").and_then(Value::as_str) != Some("object") {
        FormMode::RawJson("{}".to_owned())
    } else {
        FormMode::Fields(fields)
    }
}

pub fn build_payload(mode: &FormMode, schema: &Value) -> Result<Value> {
    let payload = match mode {
        FormMode::RawJson(text) => serde_json::from_str(text)
            .map_err(|error| Error::Validation(format!("raw input is not valid JSON: {error}")))?,
        FormMode::Fields(fields) => {
            let mut payload = Value::Object(Map::new());
            for field in fields {
                if field.value.is_empty() && !field.required {
                    continue;
                }
                let value = parse_field(field)?;
                set_path(&mut payload, &field.path, value);
            }
            payload
        }
    };
    let mut errors = Vec::new();
    validate_value(schema, &payload, "$", &mut errors);
    if errors.is_empty() {
        Ok(payload)
    } else {
        Err(Error::Validation(errors.join("; ")))
    }
}

pub fn redacted_payload(payload: &Value, schema: &Value) -> Value {
    let mut redacted = payload.clone();
    redact(schema, &mut redacted);
    redacted
}

pub fn is_destructive_method(name: &str) -> bool {
    const ACTIONS: &[&str] = &[
        "delete",
        "destroy",
        "remove",
        "terminate",
        "purge",
        "revoke",
        "stop",
    ];
    let mut normalized = String::with_capacity(name.len() * 2);
    for (index, character) in name.chars().enumerate() {
        if index > 0 && character.is_ascii_uppercase() {
            normalized.push('_');
        }
        normalized.push(character.to_ascii_lowercase());
    }
    normalized
        .split(|character: char| !character.is_ascii_alphanumeric())
        .any(|token| ACTIONS.contains(&token))
}

fn contains_unsupported(value: &Value) -> bool {
    match value {
        Value::Object(map) => {
            const KEYS: &[&str] = &["$ref", "oneOf", "anyOf", "allOf", "if", "then", "else"];
            KEYS.iter().any(|key| map.contains_key(*key)) || map.values().any(contains_unsupported)
        }
        Value::Array(values) => values.iter().any(contains_unsupported),
        _ => false,
    }
}

fn collect_fields(
    schema: &Value,
    parent: &[String],
    parent_required: bool,
    fields: &mut Vec<FormField>,
) {
    let required: BTreeSet<&str> = schema
        .get("required")
        .and_then(Value::as_array)
        .into_iter()
        .flatten()
        .filter_map(Value::as_str)
        .collect();
    let Some(properties) = schema.get("properties").and_then(Value::as_object) else {
        return;
    };
    for (name, property) in properties {
        let mut path = parent.to_vec();
        path.push(name.clone());
        let is_required = parent_required || required.contains(name.as_str());
        if property.get("type").and_then(Value::as_str) == Some("object")
            && property.get("properties").is_some()
        {
            collect_fields(property, &path, is_required, fields);
            continue;
        }
        let kind = if let Some(values) = property.get("enum").and_then(Value::as_array) {
            FieldKind::Enum(values.iter().map(value_to_input).collect())
        } else {
            match property.get("type").and_then(Value::as_str) {
                Some("string") | None => FieldKind::String,
                Some("integer") => FieldKind::Integer,
                Some("number") => FieldKind::Number,
                Some("boolean") => FieldKind::Boolean,
                Some("array" | "object") => FieldKind::Json,
                Some(_) => FieldKind::Json,
            }
        };
        let value = property
            .get("default")
            .map(value_to_input)
            .unwrap_or_else(|| match kind {
                FieldKind::Boolean => "false".to_owned(),
                _ => String::new(),
            });
        fields.push(FormField {
            path: path.clone(),
            label: path.join("."),
            description: property
                .get("description")
                .and_then(Value::as_str)
                .unwrap_or_default()
                .to_owned(),
            required: is_required,
            secret: property.get("writeOnly").and_then(Value::as_bool) == Some(true)
                || property.get("format").and_then(Value::as_str) == Some("password"),
            kind,
            value,
        });
    }
}

fn value_to_input(value: &Value) -> String {
    value
        .as_str()
        .map(str::to_owned)
        .unwrap_or_else(|| value.to_string())
}

fn parse_field(field: &FormField) -> Result<Value> {
    let invalid = |expected: &str| Error::Validation(format!("{} must be {expected}", field.label));
    match &field.kind {
        FieldKind::String => Ok(Value::String(field.value.clone())),
        FieldKind::Integer => field
            .value
            .parse::<i64>()
            .map(Value::from)
            .map_err(|_| invalid("an integer")),
        FieldKind::Number => field
            .value
            .parse::<f64>()
            .map(Value::from)
            .map_err(|_| invalid("a number")),
        FieldKind::Boolean => field
            .value
            .parse::<bool>()
            .map(Value::from)
            .map_err(|_| invalid("true or false")),
        FieldKind::Enum(values) => {
            if !values.contains(&field.value) {
                return Err(Error::Validation(format!(
                    "{} must be one of {}",
                    field.label,
                    values.join(", ")
                )));
            }
            Ok(serde_json::from_str(&field.value)
                .unwrap_or_else(|_| Value::String(field.value.clone())))
        }
        FieldKind::Json => serde_json::from_str(&field.value).map_err(|_| invalid("valid JSON")),
    }
}

fn set_path(root: &mut Value, path: &[String], value: Value) {
    let mut current = root;
    for key in &path[..path.len().saturating_sub(1)] {
        let map = current
            .as_object_mut()
            .expect("form payload paths are objects");
        current = map
            .entry(key.clone())
            .or_insert_with(|| Value::Object(Map::new()));
    }
    if let (Some(map), Some(key)) = (current.as_object_mut(), path.last()) {
        map.insert(key.clone(), value);
    }
}

fn validate_value(schema: &Value, value: &Value, path: &str, errors: &mut Vec<String>) {
    let schema_type = schema.get("type").and_then(Value::as_str);
    let correct_type = match schema_type {
        Some("object") => value.is_object(),
        Some("array") => value.is_array(),
        Some("string") => value.is_string(),
        Some("integer") => value.as_i64().is_some() || value.as_u64().is_some(),
        Some("number") => value.is_number(),
        Some("boolean") => value.is_boolean(),
        Some("null") => value.is_null(),
        Some(_) | None => true,
    };
    if !correct_type {
        errors.push(format!("{path} must be {}", schema_type.unwrap_or("valid")));
        return;
    }

    if let Some(allowed) = schema.get("enum").and_then(Value::as_array)
        && !allowed.contains(value)
    {
        errors.push(format!("{path} is not an allowed value"));
    }
    if let Some(text) = value.as_str() {
        if let Some(minimum) = schema.get("minLength").and_then(Value::as_u64)
            && text.chars().count() < minimum as usize
        {
            errors.push(format!("{path} is shorter than {minimum} characters"));
        }
        if let Some(maximum) = schema.get("maxLength").and_then(Value::as_u64)
            && text.chars().count() > maximum as usize
        {
            errors.push(format!("{path} is longer than {maximum} characters"));
        }
        if let Some(pattern) = schema.get("pattern").and_then(Value::as_str)
            && let Ok(regex) = Regex::new(pattern)
            && !regex.is_match(text)
        {
            errors.push(format!("{path} does not match {pattern}"));
        }
    }
    if let Some(number) = value.as_f64() {
        for (key, inclusive, lower) in [
            ("minimum", true, true),
            ("exclusiveMinimum", false, true),
            ("maximum", true, false),
            ("exclusiveMaximum", false, false),
        ] {
            if let Some(limit) = schema.get(key).and_then(Value::as_f64) {
                let valid = match (lower, inclusive) {
                    (true, true) => number >= limit,
                    (true, false) => number > limit,
                    (false, true) => number <= limit,
                    (false, false) => number < limit,
                };
                if !valid {
                    errors.push(format!("{path} violates {key} {limit}"));
                }
            }
        }
    }
    if let Some(object) = value.as_object() {
        let required: BTreeSet<&str> = schema
            .get("required")
            .and_then(Value::as_array)
            .into_iter()
            .flatten()
            .filter_map(Value::as_str)
            .collect();
        for key in required {
            if !object.contains_key(key) {
                errors.push(format!("{path}.{key} is required"));
            }
        }
        if let Some(properties) = schema.get("properties").and_then(Value::as_object) {
            for (key, child) in object {
                if let Some(child_schema) = properties.get(key) {
                    validate_value(child_schema, child, &format!("{path}.{key}"), errors);
                } else if schema.get("additionalProperties").and_then(Value::as_bool) == Some(false)
                {
                    errors.push(format!("{path}.{key} is not allowed"));
                }
            }
        }
    }
    if let Some(items) = schema.get("items")
        && let Some(values) = value.as_array()
    {
        for (index, item) in values.iter().enumerate() {
            validate_value(items, item, &format!("{path}[{index}]"), errors);
        }
    }
}

fn redact(schema: &Value, value: &mut Value) {
    if schema.get("writeOnly").and_then(Value::as_bool) == Some(true)
        || schema.get("format").and_then(Value::as_str) == Some("password")
    {
        *value = Value::String("<redacted>".to_owned());
        return;
    }
    if let (Some(properties), Some(object)) = (
        schema.get("properties").and_then(Value::as_object),
        value.as_object_mut(),
    ) {
        for (key, child) in object {
            if let Some(child_schema) = properties.get(key) {
                redact(child_schema, child);
            }
        }
    }
    if let (Some(items), Some(values)) = (schema.get("items"), value.as_array_mut()) {
        for item in values {
            redact(items, item);
        }
    }
}

#[cfg(test)]
mod tests {
    use serde_json::json;

    use super::*;

    #[test]
    fn builds_and_validates_common_fields() {
        let schema = json!({
            "type": "object",
            "properties": {
                "name": {"type": "string", "minLength": 2},
                "count": {"type": "integer", "minimum": 1},
                "enabled": {"type": "boolean"}
            },
            "required": ["name", "count"],
            "additionalProperties": false
        });
        let FormMode::Fields(mut fields) = form_for(&schema) else {
            panic!("expected fields")
        };
        fields.iter_mut().find(|f| f.label == "name").unwrap().value = "ok".into();
        fields
            .iter_mut()
            .find(|f| f.label == "count")
            .unwrap()
            .value = "2".into();
        assert_eq!(
            build_payload(&FormMode::Fields(fields), &schema).unwrap(),
            json!({"name": "ok", "count": 2, "enabled": false})
        );
    }

    #[test]
    fn redacts_schema_marked_secrets() {
        let schema = json!({"type":"object","properties":{
            "token":{"type":"string","writeOnly":true},
            "name":{"type":"string"}
        }});
        assert_eq!(
            redacted_payload(&json!({"token":"secret","name":"visible"}), &schema),
            json!({"token":"<redacted>","name":"visible"})
        );
    }

    #[test]
    fn destructive_detection_uses_tokens() {
        assert!(is_destructive_method("destroy-instance"));
        assert!(is_destructive_method("destroyInstance"));
        assert!(!is_destructive_method("restore"));
    }
}
