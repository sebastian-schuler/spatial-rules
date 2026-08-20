//! Immutable `Ruleset` compilation and the batch query engine
//! (ADR-0001/0002/0003/0004/0005).

use std::collections::{BTreeMap, HashMap, HashSet};

use geo::{BoundingRect, Geometry, PreparedGeometry, Rect};

use crate::candidate::Candidate;
pub use crate::evaluate::PreparedQuery;
use crate::error::{ErrorCode, SpatialError};
use crate::prepared_cache::{get_or_prepare, next_ruleset_id, PreparedGeometries};
use crate::property_index::{EqualityIndex, PropertyIndex};
use crate::properties::PropertyValue;
use crate::query::{CandidateOutcome, Query};
use crate::rule::{Rule, RuleId};
use crate::spatial_index::{build_spatial_index, SpatialIndex, SpatialIndexKind};
use crate::validation::validate_rule_geometry;

/// An immutable, query-optimized collection of rules (CONTEXT.md §6).
///
/// Fully built before publication and never mutated afterwards; shared across
/// requests behind an `Arc`.
pub struct Ruleset {
    /// Unique identity of this compiled ruleset instance (distinct from
    /// `Engine`'s replacement `version`) — the prepared-geometry cache key.
    id: u64,
    rules: Vec<Rule>,
    ids: HashMap<String, RuleId>,
    envelopes: Vec<Rect<f64>>,
    spatial_index: Box<dyn SpatialIndex>,
    property_index: Box<dyn PropertyIndex>,
}

impl std::fmt::Debug for Ruleset {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        let mut ids: Vec<&String> = self.ids.keys().collect();
        ids.sort();
        f.debug_struct("Ruleset")
            .field("rule_count", &self.rules.len())
            .field("rule_ids", &ids)
            .finish()
    }
}

impl Ruleset {
    /// Parse a GeoJSON FeatureCollection and build a ruleset from it.
    pub fn from_geojson(input: &str) -> Result<Self, SpatialError> {
        let rules = crate::ingestion::rules_from_geojson(input)?;
        Self::build(rules)
    }

    /// Build a ruleset with the default spatial index (`rstar`, ADR-0002).
    pub fn build(rules: Vec<Rule>) -> Result<Self, SpatialError> {
        Self::build_with(rules, SpatialIndexKind::RStar)
    }

    /// Build a ruleset with an explicit spatial index (benchmark ladder).
    pub fn build_with(
        rules: Vec<Rule>,
        index_kind: SpatialIndexKind,
    ) -> Result<Self, SpatialError> {
        let mut ids = HashMap::with_capacity(rules.len());
        for (index, rule) in rules.iter().enumerate() {
            validate_rule_geometry(&rule.geometry).map_err(|e| {
                SpatialError::new(e.code, format!("rule '{}': {}", rule.id, e.message))
            })?;
            if ids.insert(rule.id.clone(), RuleId(index as u32)).is_some() {
                return Err(SpatialError::new(
                    ErrorCode::RulesetConstructionFailed,
                    format!("duplicate rule id: '{}'", rule.id),
                ));
            }
        }

        let envelopes: Vec<Rect<f64>> = rules
            .iter()
            .map(|rule| {
                rule.geometry.bounding_rect().ok_or_else(|| {
                    SpatialError::new(
                        ErrorCode::RulesetConstructionFailed,
                        format!("rule '{}' has no bounding rectangle", rule.id),
                    )
                })
            })
            .collect::<Result<_, _>>()?;

        let index_entries: Vec<(Rect<f64>, RuleId)> = envelopes
            .iter()
            .copied()
            .enumerate()
            .map(|(index, rect)| (rect, RuleId(index as u32)))
            .collect();

        let spatial_index = build_spatial_index(index_kind, index_entries);
        let property_index: Box<dyn PropertyIndex> = Box::new(EqualityIndex::build(&rules));

        Ok(Ruleset {
            id: next_ruleset_id(),
            rules,
            ids,
            envelopes,
            spatial_index,
            property_index,
        })
    }

    /// Number of rules in the ruleset.
    pub fn len(&self) -> usize {
        self.rules.len()
    }

    /// Whether the ruleset has no rules.
    pub fn is_empty(&self) -> bool {
        self.rules.is_empty()
    }

    /// Map an application-supplied string id to its numeric [`RuleId`].
    pub fn rule_id(&self, string_id: &str) -> Option<RuleId> {
        self.ids.get(string_id).copied()
    }

    /// Map a numeric [`RuleId`] back to the application-supplied string id.
    ///
    /// Panics if `rule_id` is out of range (rule ids are only produced by this
    /// ruleset).
    pub fn string_id(&self, rule_id: RuleId) -> &str {
        &self.rules[rule_id.0 as usize].id
    }

    /// The geometry of a rule by [`RuleId`].
    pub fn geometry(&self, rule_id: RuleId) -> &Geometry<f64> {
        &self.rules[rule_id.0 as usize].geometry
    }

    /// The properties of a rule by [`RuleId`].
    pub fn properties(&self, rule_id: RuleId) -> &BTreeMap<String, PropertyValue> {
        &self.rules[rule_id.0 as usize].properties
    }

    /// The precomputed envelope of a rule by [`RuleId`].
    pub fn envelope(&self, rule_id: RuleId) -> &Rect<f64> {
        &self.envelopes[rule_id.0 as usize]
    }

    /// Rule ids whose envelope intersects `envelope`, via the spatial index.
    pub fn query_envelope(&self, envelope: &Rect<f64>) -> Vec<RuleId> {
        self.spatial_index.query_envelope(envelope)
    }

    /// Fill `out` with the rule ids whose envelope intersects `envelope`
    /// (sorted ascending, deduplicated) — the reusable form a batch query uses
    /// so the per-candidate allocation moves out of the hot loop
    /// (architecture-hardening 03).
    pub fn query_envelope_into(&self, envelope: &Rect<f64>, out: &mut Vec<RuleId>) {
        self.spatial_index.query_envelope_into(envelope, out)
    }

    /// Serialize the canonical **rules** (not the compiled indexes) to JSON
    /// bytes (ADR-0013). Deterministic: properties are a sorted `BTreeMap` and
    /// geometry is the validated/canonicalized `geo` geometry.
    pub fn to_canonical(&self) -> Result<Vec<u8>, SpatialError> {
        serde_json::to_vec(&self.rules).map_err(|e| {
            SpatialError::new(
                ErrorCode::Native,
                format!("serialize canonical ruleset: {e}"),
            )
        })
    }

    /// Load a ruleset from canonical JSON bytes, re-running the full build
    /// (validation, envelopes, rstar index, property index) and assigning a
    /// **fresh `Ruleset.id`** — the id is never persisted (ADR-0010, ADR-0013).
    pub fn from_canonical(input: &[u8]) -> Result<Self, SpatialError> {
        let rules: Vec<Rule> = serde_json::from_slice(input).map_err(|e| {
            SpatialError::invalid_geojson(format!("failed to parse canonical ruleset: {e}"))
        })?;
        Self::build(rules)
    }

    /// Rules in ruleset order, via a [`RuleSource`] — the seam the benchmark
    /// ladder consumes (ADR-0002). It replaces the two ladder-only positional
    /// accessors (`rule_ids`/`rule_geometries`); the per-id accessors
    /// (`geometry`, `envelope`, `properties`) remain for the binding.
    pub fn rules(&self) -> RuleSource<'_> {
        RuleSource {
            rules: &self.rules,
            envelopes: &self.envelopes,
        }
    }

    /// The prepared rule geometries for this ruleset, cached per thread
    /// (ADR-0010). A handle indexed by opaque [`RuleId`], so callers (the
    /// benchmark ladder) never reconstruct a positional id-to-index map
    /// (architecture-hardening 04). Cloning the handle is cheap (`Rc`).
    pub fn prepared(&self) -> PreparedRuleGeometries {
        PreparedRuleGeometries {
            inner: get_or_prepare(&self.rules, self.id),
        }
    }

    /// Evaluate a batch of candidates against `query`, returning one outcome
    /// per candidate in input order (ADR-0004). Invalid candidates produce an
    /// [`CandidateOutcome::Invalid`] outcome without failing the batch
    /// (ADR-0005).
    pub fn query(&self, candidates: &[Candidate], query: &Query) -> Vec<CandidateOutcome> {
        let prepared = self.prepare(query);
        candidates
            .iter()
            .map(|candidate| prepared.evaluate(candidate))
            .collect()
    }

    /// Evaluate a batch and return the compact mask (`0` no match, `1`
    /// matched, `2` invalid), without materialising per-match rule ids
    /// (ADR-0004).
    pub fn query_mask(&self, candidates: &[Candidate], query: &Query) -> Vec<u8> {
        let prepared = self.prepare(query);
        candidates
            .iter()
            .map(|candidate| prepared.evaluate_mask(candidate))
            .collect()
    }

    /// Compile a query into a reusable [`PreparedQuery`] holding the preparation:
    /// excluded ids, prepared rule geometries (cached per thread, ADR-0010), and
    /// the indexable `where` set. [`PreparedQuery::evaluate`] and
    /// [`PreparedQuery::evaluate_mask`] share this one preparation across the
    /// whole candidate batch. This is the planner hook ADR-0003 reserves — a
    /// cost-based planner would return a differently-shaped query here.
    pub fn prepare<'a>(&'a self, query: &Query) -> PreparedQuery<'a> {
        let excluded: HashSet<RuleId> = query
            .exclude_rule_ids
            .iter()
            .filter_map(|id| self.rule_id(id))
            .collect();
        let prepared = get_or_prepare(&self.rules, self.id);
        let where_filter = query
            .where_clause
            .as_ref()
            .and_then(|where_clause| self.property_index.indexable_matches(where_clause));
        PreparedQuery::new(self, query, excluded, prepared, where_filter)
    }
}

/// Iteration over rules in ruleset order: id, geometry, and precomputed
/// envelope per rule. The seam the benchmark ladder consumes (ADR-0002) — it
/// replaces raw positional accessors, so the ruleset stops advertising its
/// storage layout.
pub struct RuleSource<'a> {
    rules: &'a [Rule],
    envelopes: &'a [Rect<f64>],
}

impl<'a> RuleSource<'a> {
    /// Iterate over `(id, geometry, envelope)` in ruleset order.
    pub fn iter(&self) -> impl Iterator<Item = (RuleId, &'a Geometry<f64>, &'a Rect<f64>)> {
        self.rules
            .iter()
            .enumerate()
            .map(|(index, rule)| (RuleId(index as u32), &rule.geometry, &self.envelopes[index]))
    }
}

/// A handle to one ruleset's prepared rule geometries (ADR-0010), indexed by
/// opaque [`RuleId`]. Callers fetch a rule's prepared form by id without ever
/// reading the numeric position (architecture-hardening 04).
pub struct PreparedRuleGeometries {
    inner: PreparedGeometries,
}

impl PreparedRuleGeometries {
    /// The prepared DE-9IM geometry for a rule by opaque [`RuleId`].
    pub fn get(&self, rule_id: RuleId) -> &PreparedGeometry<'static, Geometry<f64>> {
        &self.inner[rule_id.0 as usize]
    }

    /// Iterate over prepared geometries in ruleset order.
    pub fn iter(&self) -> impl Iterator<Item = &PreparedGeometry<'static, Geometry<f64>>> {
        self.inner.iter()
    }

    /// Number of prepared geometries (== the ruleset's rule count).
    pub fn len(&self) -> usize {
        self.inner.len()
    }

    /// Whether the prepared geometry set is empty.
    pub fn is_empty(&self) -> bool {
        self.inner.is_empty()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use geo::LineString;

    fn sample_rules() -> Vec<Rule> {
        vec![Rule {
            id: "zone".to_string(),
            properties: BTreeMap::new(),
            geometry: Geometry::Polygon(geo::Polygon::new(
                LineString::from(vec![(0.0, 0.0), (0.0, 1.0), (1.0, 1.0), (1.0, 0.0), (0.0, 0.0)]),
                vec![],
            )),
        }]
    }

    #[test]
    fn from_canonical_assigns_a_fresh_id() {
        let original = Ruleset::build(sample_rules()).unwrap();
        let bytes = original.to_canonical().unwrap();

        let loaded = Ruleset::from_canonical(&bytes).unwrap();
        assert_ne!(original.id, loaded.id);

        // Two loads from the same bytes are also distinct instances.
        let loaded_again = Ruleset::from_canonical(&bytes).unwrap();
        assert_ne!(loaded.id, loaded_again.id);
    }
}
