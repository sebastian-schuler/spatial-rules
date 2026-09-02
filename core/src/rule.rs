//! Rule type and numeric rule-id mapping (ADR-0004).

use std::collections::BTreeMap;

use geo::Geometry;

use crate::properties::PropertyValue;

/// Internal numeric identifier for a rule, assigned `0..n-1` at ruleset build.
///
/// The numeric index is exposed read-only for stable serialization and
/// cross-ruleset parity comparison, but callers should otherwise treat a
/// [`RuleId`] as an opaque handle obtained from the owning [`Ruleset`]. The
/// `owner` field — private and deliberately excluded from equality, ordering,
/// and hashing — binds the id to the one ruleset instance that minted it, so
/// passing an id from another ruleset can never silently select an unrelated
/// rule at the same position. Owner-insensitive equality keeps ids from two
/// independently-built rulesets comparable by position (index-kind parity
/// tests), while the owning `Ruleset` still rejects a foreign id at the access
/// boundary via the owner it stores.
#[derive(Debug, Clone, Copy)]
pub struct RuleId {
    pub(crate) index: u32,
    /// The owning ruleset id this id was minted from (see `Ruleset::id`).
    pub(crate) owner: u64,
}

impl RuleId {
    /// Bind a positional index to its owning ruleset id (crate-internal).
    pub(crate) fn new(index: u32, owner: u64) -> Self {
        RuleId { index, owner }
    }

    /// The positional index this id was assigned from, for stable ordering and
    /// cross-ruleset parity comparison (owner-insensitive).
    pub fn index(self) -> usize {
        self.index as usize
    }
}

impl PartialEq for RuleId {
    fn eq(&self, other: &Self) -> bool {
        self.index == other.index
    }
}

impl Eq for RuleId {}

impl PartialOrd for RuleId {
    fn partial_cmp(&self, other: &Self) -> Option<std::cmp::Ordering> {
        Some(self.cmp(other))
    }
}

impl Ord for RuleId {
    fn cmp(&self, other: &Self) -> std::cmp::Ordering {
        self.index.cmp(&other.index)
    }
}

impl std::hash::Hash for RuleId {
    fn hash<H: std::hash::Hasher>(&self, state: &mut H) {
        self.index.hash(state);
    }
}

/// A geometry-bearing rule with queryable properties (CONTEXT.md §4.1).
#[derive(Debug, Clone, PartialEq, serde::Serialize, serde::Deserialize)]
pub struct Rule {
    /// Application-supplied identifier; internally mapped to a [`RuleId`].
    pub id: String,
    /// Compact typed property storage (ADR-0003).
    pub properties: BTreeMap<String, PropertyValue>,
    /// The rule's geometry (Polygon or MultiPolygon once validated).
    pub geometry: Geometry<f64>,
    /// Top-level precedence for resolution (ADR-0015): higher wins; a missing
    /// field is `0` (unprioritized rules sort below any explicit priority).
    /// A `priority` inside `properties` is plain metadata, never read here.
    #[serde(default)]
    pub priority: i64,
}
