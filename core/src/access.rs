//! Read-only ruleset access seam (architecture hardening).
//!
//! Evaluation and aggregation depend on this narrow read-only view instead of
//! the concrete [`Ruleset`], so the module graph stays acyclic:
//! `ruleset → evaluate → aggregate`, with `ruleset` implementing this seam and
//! neither `evaluate` nor `aggregate` importing `ruleset`. This keeps the
//! immutable ruleset's storage layout private while giving the query hot path
//! the exact methods it needs.
//!
//! [`Ruleset`]: crate::ruleset::Ruleset

use std::collections::BTreeMap;

use geo::{Geometry, Rect};

use crate::properties::PropertyValue;
use crate::rule::RuleId;

/// The read-only rule access operations the evaluation and aggregation paths
/// need. Implemented by [`Ruleset`]; the query hot path and the aggregate
/// engine consume it as `&dyn RuleAccess` so they never depend on the concrete
/// ruleset type.
pub trait RuleAccess {
    /// Fill `out` with the rule ids whose envelope intersects `envelope`
    /// (sorted ascending, deduplicated).
    fn query_envelope_into(&self, envelope: &Rect<f64>, out: &mut Vec<RuleId>);

    /// The geometry of a rule by opaque [`RuleId`].
    fn geometry(&self, rule_id: RuleId) -> &Geometry<f64>;

    /// The typed properties of a rule by opaque [`RuleId`].
    fn properties(&self, rule_id: RuleId) -> &BTreeMap<String, PropertyValue>;

    /// The top-level precedence of a rule by opaque [`RuleId`] (ADR-0015).
    fn priority(&self, rule_id: RuleId) -> i64;
}
