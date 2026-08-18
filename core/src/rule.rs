//! Rule type and numeric rule-id mapping (ADR-0004).

use std::collections::BTreeMap;

use geo::Geometry;

use crate::properties::PropertyValue;

/// Internal numeric identifier for a rule, assigned `0..n-1` at ruleset build.
///
/// The numeric field is crate-private: callers treat a [`RuleId`] as an opaque
/// handle obtained from the ruleset, never as a position. Only `Ruleset` and
/// its compile-time indexes may reconstruct or read the value.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct RuleId(pub(crate) u32);

/// A geometry-bearing rule with queryable properties (CONTEXT.md §4.1).
#[derive(Debug, Clone, PartialEq)]
pub struct Rule {
    /// Application-supplied identifier; internally mapped to a [`RuleId`].
    pub id: String,
    /// Compact typed property storage (ADR-0003).
    pub properties: BTreeMap<String, PropertyValue>,
    /// The rule's geometry (Polygon or MultiPolygon once validated).
    pub geometry: Geometry<f64>,
}
