//! Compact typed storage for rule properties (ADR-0003).

use std::collections::BTreeMap;
use std::fmt;

/// A single queryable property value, stored compactly and typed (ADR-0003).
///
/// JSON numbers become [`PropertyValue::Int`] when integral and
/// [`PropertyValue::Float`] otherwise. Nested objects and arrays are out of
/// scope for v1 and are not stored.
#[derive(Debug, Clone, PartialEq)]
pub enum PropertyValue {
    Null,
    Bool(bool),
    Int(i64),
    Float(f64),
    Str(String),
}

impl PropertyValue {
    /// Convert a JSON value into a [`PropertyValue`], or `None` for the
    /// unsupported v1 types (arrays and nested objects).
    pub fn from_json_value(value: &serde_json::Value) -> Option<Self> {
        match value {
            serde_json::Value::Null => Some(PropertyValue::Null),
            serde_json::Value::Bool(b) => Some(PropertyValue::Bool(*b)),
            serde_json::Value::Number(n) => number_to_property_value(n),
            serde_json::Value::String(s) => Some(PropertyValue::Str(s.clone())),
            serde_json::Value::Array(_) | serde_json::Value::Object(_) => None,
        }
    }
}

/// Map a JSON number to a [`PropertyValue`]: `Int` when the value is integral,
/// else `Float` (ADR-0003). serde_json keeps lexical integer/float apart, so an
/// integral float like `10.0` is re-examined by value rather than by spelling.
fn number_to_property_value(n: &serde_json::Number) -> Option<PropertyValue> {
    if let Some(i) = n.as_i64() {
        return Some(PropertyValue::Int(i));
    }
    if let Some(f) = n.as_f64() {
        // 2^63 is the exclusive upper bound; -2^63 is i64::MIN, exactly
        // representable as f64. Values in [-2^63, 2^63) cast without overflow.
        const I64_MIN: f64 = i64::MIN as f64;
        const I64_MAX_EXCLUSIVE: f64 = 9_223_372_036_854_775_808.0;
        if f.is_finite() && f.fract() == 0.0 && (I64_MIN..I64_MAX_EXCLUSIVE).contains(&f) {
            return Some(PropertyValue::Int(f as i64));
        }
        return Some(PropertyValue::Float(f));
    }
    None
}

impl fmt::Display for PropertyValue {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            PropertyValue::Null => write!(f, "null"),
            PropertyValue::Bool(b) => write!(f, "{b}"),
            PropertyValue::Int(i) => write!(f, "{i}"),
            PropertyValue::Float(v) => write!(f, "{v}"),
            PropertyValue::Str(s) => write!(f, "{s}"),
        }
    }
}

/// Convert a feature's JSON properties into compact typed storage, skipping
/// the unsupported v1 value types (nested objects/arrays).
pub fn properties_from_json(
    map: &serde_json::Map<String, serde_json::Value>,
) -> BTreeMap<String, PropertyValue> {
    let mut properties = BTreeMap::new();
    for (key, value) in map {
        if let Some(property_value) = PropertyValue::from_json_value(value) {
            properties.insert(key.clone(), property_value);
        }
    }
    properties
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn scalar_json_values_map_to_typed_values() {
        assert_eq!(
            PropertyValue::from_json_value(&json!(null)),
            Some(PropertyValue::Null)
        );
        assert_eq!(
            PropertyValue::from_json_value(&json!(true)),
            Some(PropertyValue::Bool(true))
        );
        assert_eq!(
            PropertyValue::from_json_value(&json!(10)),
            Some(PropertyValue::Int(10))
        );
        assert_eq!(
            PropertyValue::from_json_value(&json!(4.2)),
            Some(PropertyValue::Float(4.2))
        );
        assert_eq!(
            PropertyValue::from_json_value(&json!("HR")),
            Some(PropertyValue::Str("HR".to_string()))
        );
    }

    #[test]
    fn integral_floats_become_int() {
        assert_eq!(
            PropertyValue::from_json_value(&json!(10.0)),
            Some(PropertyValue::Int(10))
        );
    }

    #[test]
    fn nested_objects_and_arrays_are_not_stored() {
        assert_eq!(PropertyValue::from_json_value(&json!({"a": 1})), None);
        assert_eq!(PropertyValue::from_json_value(&json!([1, 2])), None);
    }

    #[test]
    fn properties_from_json_skips_unsupported_values() {
        let map = serde_json::Map::from_iter([
            ("active".to_string(), json!(true)),
            ("nested".to_string(), json!({"a": 1})),
        ]);
        let properties = properties_from_json(&map);
        assert_eq!(properties.len(), 1);
        assert_eq!(properties.get("active"), Some(&PropertyValue::Bool(true)));
        assert!(!properties.contains_key("nested"));
    }
}
