//! Compact typed storage for rule properties (ADR-0003).

use std::fmt;

/// A single queryable property value, stored compactly and typed (ADR-0003).
///
/// JSON numbers become [`PropertyValue::Int`] when integral and
/// [`PropertyValue::Float`] otherwise. Nested objects and arrays are out of
/// scope for v1 and are not stored.
///
/// Serializes to the plain JSON scalar it wraps (ADR-0013: canonical ruleset
/// persistence), so a ruleset's properties round-trip as `null`/`true`/`10`/
/// `1.5`/`"x"` rather than an externally-tagged enum shape.
#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
#[serde(untagged)]
pub enum PropertyValue {
    Null,
    Bool(bool),
    Int(i64),
    Float(f64),
    Str(String),
}

// Floats are only ever non-integral and finite (integral JSON numbers become
// `Int`; serde_json rejects non-finite values), so bit equality equals value
// equality and `to_bits()` gives a total order consistent with `Eq`.
impl PartialEq for PropertyValue {
    fn eq(&self, other: &Self) -> bool {
        match (self, other) {
            (PropertyValue::Null, PropertyValue::Null) => true,
            (PropertyValue::Bool(a), PropertyValue::Bool(b)) => a == b,
            (PropertyValue::Int(a), PropertyValue::Int(b)) => a == b,
            (PropertyValue::Float(a), PropertyValue::Float(b)) => a.to_bits() == b.to_bits(),
            (PropertyValue::Str(a), PropertyValue::Str(b)) => a == b,
            _ => false,
        }
    }
}

impl Eq for PropertyValue {}

impl PartialOrd for PropertyValue {
    fn partial_cmp(&self, other: &Self) -> Option<std::cmp::Ordering> {
        Some(self.cmp(other))
    }
}

impl Ord for PropertyValue {
    fn cmp(&self, other: &Self) -> std::cmp::Ordering {
        use std::cmp::Ordering;
        match (self, other) {
            (PropertyValue::Null, PropertyValue::Null) => Ordering::Equal,
            (PropertyValue::Null, _) => Ordering::Less,
            (_, PropertyValue::Null) => Ordering::Greater,
            (PropertyValue::Bool(a), PropertyValue::Bool(b)) => a.cmp(b),
            (PropertyValue::Bool(_), _) => Ordering::Less,
            (_, PropertyValue::Bool(_)) => Ordering::Greater,
            (PropertyValue::Int(a), PropertyValue::Int(b)) => a.cmp(b),
            (PropertyValue::Int(_), _) => Ordering::Less,
            (_, PropertyValue::Int(_)) => Ordering::Greater,
            (PropertyValue::Float(a), PropertyValue::Float(b)) => a.to_bits().cmp(&b.to_bits()),
            (PropertyValue::Float(_), _) => Ordering::Less,
            (_, PropertyValue::Float(_)) => Ordering::Greater,
            (PropertyValue::Str(a), PropertyValue::Str(b)) => a.cmp(b),
        }
    }
}

impl std::hash::Hash for PropertyValue {
    fn hash<H: std::hash::Hasher>(&self, state: &mut H) {
        std::mem::discriminant(self).hash(state);
        match self {
            PropertyValue::Null => {}
            PropertyValue::Bool(b) => b.hash(state),
            PropertyValue::Int(i) => i.hash(state),
            PropertyValue::Float(f) => f.to_bits().hash(state),
            PropertyValue::Str(s) => s.hash(state),
        }
    }
}

impl PropertyValue {
    /// Build a [`PropertyValue::Float`], rejecting the non-finite floats
    /// (`NaN`/`±inf`) the canonical JSON serialization cannot represent. Returns
    /// `None` for a non-finite value so a programmatic caller cannot construct a
    /// value that would later fail at the serialization boundary.
    pub fn float(value: f64) -> Option<Self> {
        if value.is_finite() {
            Some(PropertyValue::Float(value))
        } else {
            None
        }
    }

    /// Whether this value is one the canonical ruleset JSON can serialize — in
    /// v1 that means no non-finite `Float` (arrays/nested objects are never
    /// stored). Used to validate a ruleset at construction so every path that
    /// builds one (GeoJSON, canonical, programmatic) lands on the same gate.
    pub fn is_serializable(&self) -> bool {
        match self {
            PropertyValue::Float(f) => f.is_finite(),
            _ => true,
        }
    }

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

/// A rule's queryable properties: an immutable, key-sorted, compact map
/// (ADR-0003).
///
/// Replaces `BTreeMap<String, PropertyValue>`, whose first insert allocates a
/// leaf node sized for 11 entries (~600 B) whatever the property count
/// (perf-memory 03). Rules are immutable after build, so a sorted
/// `Box<[(Box<str>, PropertyValue)]>` gives `O(log n)` lookup and pays only for
/// the properties actually present. Ordering is by key, so canonical JSON output
/// is byte-identical to the `BTreeMap` it replaced.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct Properties(Box<[(Box<str>, PropertyValue)]>);

impl Properties {
    /// Build from `(name, value)` pairs. Keys must be unique (the sources —
    /// a JSON object, a test literal — always are); ordering is applied here.
    pub fn from_pairs<K, I>(pairs: I) -> Self
    where
        K: AsRef<str>,
        I: IntoIterator<Item = (K, PropertyValue)>,
    {
        let mut items: Vec<(Box<str>, PropertyValue)> = pairs
            .into_iter()
            .map(|(name, value)| (Box::<str>::from(name.as_ref()), value))
            .collect();
        Self::sort(&mut items);
        Self(items.into_boxed_slice())
    }

    fn sort(items: &mut [(Box<str>, PropertyValue)]) {
        items.sort_unstable_by(|a, b| a.0.as_ref().cmp(b.0.as_ref()));
    }

    /// The value for `name`, or `None` when the rule does not define it.
    pub fn get(&self, name: &str) -> Option<&PropertyValue> {
        self.0
            .binary_search_by(|(key, _)| key.as_ref().cmp(name))
            .ok()
            .map(|index| &self.0[index].1)
    }

    /// Whether the rule defines `name`.
    pub fn contains_key(&self, name: &str) -> bool {
        self.get(name).is_some()
    }

    /// Iterate `(name, value)` in key order.
    pub fn iter(&self) -> impl Iterator<Item = (&str, &PropertyValue)> {
        self.0.iter().map(|(name, value)| (name.as_ref(), value))
    }

    /// The property names, in key order.
    pub fn keys(&self) -> impl Iterator<Item = &str> {
        self.0.iter().map(|(name, _)| name.as_ref())
    }

    /// Number of properties.
    pub fn len(&self) -> usize {
        self.0.len()
    }

    /// Whether the rule has no properties.
    pub fn is_empty(&self) -> bool {
        self.0.is_empty()
    }

    /// Insert or replace a property, keeping the map ordered.
    ///
    /// A publication-time builder: `Ruleset::build` freezes properties, so this
    /// is for constructing rules before they are handed to the engine. Takes an
    /// owned name, matching the `BTreeMap::insert` it replaces.
    pub fn insert(&mut self, name: String, value: PropertyValue) {
        match self.0.binary_search_by(|(key, _)| key.as_ref().cmp(name.as_str())) {
            Ok(index) => self.0[index].1 = value,
            Err(index) => {
                let mut items = self.0.to_vec();
                items.insert(index, (Box::<str>::from(name), value));
                self.0 = items.into_boxed_slice();
            }
        }
    }
}

impl<K: AsRef<str>> FromIterator<(K, PropertyValue)> for Properties {
    fn from_iter<I: IntoIterator<Item = (K, PropertyValue)>>(iter: I) -> Self {
        Self::from_pairs(iter)
    }
}

/// Iterator over a [`Properties`] map's `(name, value)` pairs.
pub struct PropertiesIter<'a>(std::slice::Iter<'a, (Box<str>, PropertyValue)>);

impl<'a> Iterator for PropertiesIter<'a> {
    type Item = (&'a str, &'a PropertyValue);

    fn next(&mut self) -> Option<Self::Item> {
        self.0.next().map(|(name, value)| (name.as_ref(), value))
    }

    fn size_hint(&self) -> (usize, Option<usize>) {
        self.0.size_hint()
    }
}

impl<'a> IntoIterator for &'a Properties {
    type Item = (&'a str, &'a PropertyValue);
    type IntoIter = PropertiesIter<'a>;

    fn into_iter(self) -> Self::IntoIter {
        PropertiesIter(self.0.iter())
    }
}

/// Serializes as a JSON object, preserving the canonical ruleset format
/// (ADR-0013). Keys are already sorted, so output is deterministic.
impl serde::Serialize for Properties {
    fn serialize<S>(&self, serializer: S) -> Result<S::Ok, S::Error>
    where
        S: serde::Serializer,
    {
        use serde::ser::SerializeMap;
        let mut map = serializer.serialize_map(Some(self.0.len()))?;
        for (name, value) in self.0.iter() {
            map.serialize_entry(name.as_ref(), value)?;
        }
        map.end()
    }
}

impl<'de> serde::Deserialize<'de> for Properties {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: serde::Deserializer<'de>,
    {
        struct PropertiesVisitor;

        impl<'de> serde::de::Visitor<'de> for PropertiesVisitor {
            type Value = Properties;

            fn expecting(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
                f.write_str("a map of rule properties")
            }

            fn visit_map<A>(self, mut access: A) -> Result<Properties, A::Error>
            where
                A: serde::de::MapAccess<'de>,
            {
                let mut items: Vec<(Box<str>, PropertyValue)> =
                    Vec::with_capacity(access.size_hint().unwrap_or(0));
                while let Some((name, value)) = access.next_entry::<String, PropertyValue>()? {
                    items.push((name.into_boxed_str(), value));
                }
                Properties::sort(&mut items);
                Ok(Properties(items.into_boxed_slice()))
            }
        }

        deserializer.deserialize_map(PropertiesVisitor)
    }
}

/// Convert a feature's JSON properties into compact typed storage, skipping
/// the unsupported v1 value types (nested objects/arrays).
pub fn properties_from_json(map: &serde_json::Map<String, serde_json::Value>) -> Properties {
    let items: Vec<(Box<str>, PropertyValue)> = map
        .iter()
        .filter_map(|(key, value)| {
            PropertyValue::from_json_value(value).map(|v| (Box::<str>::from(key.as_str()), v))
        })
        .collect();
    Properties::from_pairs(items)
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

    #[test]
    fn float_rejects_non_finite_values() {
        assert_eq!(PropertyValue::float(4.2), Some(PropertyValue::Float(4.2)));
        assert_eq!(PropertyValue::float(f64::NAN), None);
        assert_eq!(PropertyValue::float(f64::INFINITY), None);
        assert_eq!(PropertyValue::float(f64::NEG_INFINITY), None);
    }

    #[test]
    fn is_serializable_rejects_non_finite_floats() {
        assert!(PropertyValue::Float(1.5).is_serializable());
        assert!(!PropertyValue::Float(f64::NAN).is_serializable());
        assert!(!PropertyValue::Float(f64::INFINITY).is_serializable());
    }
}
