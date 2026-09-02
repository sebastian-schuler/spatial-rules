//! Immutable `Ruleset` compilation and the batch query engine
//! (ADR-0001/0002/0003/0004/0005).

use std::collections::{BTreeMap, HashMap, HashSet};

use geo::{BoundingRect, Geometry, PreparedGeometry, Rect};

use crate::candidate::Candidate;
pub use crate::evaluate::PreparedQuery;
use crate::error::{ErrorCode, SpatialError};
use crate::prepared_cache::{next_ruleset_id, PreparedGeometries, PreparedMemo};
use crate::property_index::{EqualityIndex, PropertyIndex};
use crate::properties::PropertyValue;
use crate::query::{CandidateOutcome, Query, ResolutionOutcome};
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
    /// Hoisted top-level priorities, aligned to [`RuleId`] — the resolution
    /// path reads precedence without touching `properties` per candidate
    /// (ADR-0015).
    priorities: Vec<i64>,
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
        // Assign the ruleset identity before minting any RuleId so each id is
        // bound to this instance (a foreign id passed to an accessor is then
        // a detectable misuse, not a silent positional alias).
        let id = next_ruleset_id();
        let owner = |index: u32| RuleId::new(index, id);
        let mut ids = HashMap::with_capacity(rules.len());
        for (index, rule) in rules.iter().enumerate() {
            validate_rule_geometry(&rule.geometry).map_err(|e| {
                SpatialError::new(e.code, format!("rule '{}': {}", rule.id, e.message))
            })?;
            // Non-negativity is the authoritative precedence gate (ADR-0015):
            // every construction path — GeoJSON, canonical, and programmatic
            // `Rule`s — lands here, so a negative priority can never silently
            // sort below unprioritized (0) rules.
            if rule.priority < 0 {
                return Err(SpatialError::new(
                    ErrorCode::RulesetConstructionFailed,
                    format!(
                        "rule '{}': priority must be non-negative, found {}",
                        rule.id, rule.priority
                    ),
                ));
            }
            // Property floats must be finite so the canonical ruleset JSON (the
            // serialization boundary ADR-0013 defines) can always round-trip.
            // Enforced at the single construction gate so GeoJSON, canonical
            // loads, and programmatic `Rule`s all land here.
            if let Some((key, _)) = rule
                .properties
                .iter()
                .find(|(_, value)| !value.is_serializable())
            {
                return Err(SpatialError::new(
                    ErrorCode::RulesetConstructionFailed,
                    format!(
                        "rule '{}': property '{key}' holds a non-finite number",
                        rule.id
                    ),
                ));
            }
            if ids.insert(rule.id.clone(), owner(index as u32)).is_some() {
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
            .map(|(index, rect)| (rect, owner(index as u32)))
            .collect();

        let spatial_index = build_spatial_index(index_kind, index_entries);
        let property_index: Box<dyn PropertyIndex> = Box::new(EqualityIndex::build(&rules, id));
        let priorities: Vec<i64> = rules.iter().map(|rule| rule.priority).collect();

        Ok(Ruleset {
            id,
            rules,
            ids,
            priorities,
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

    /// The unique identity of this compiled ruleset instance — the owner every
    /// [`RuleId`] minted by this ruleset is bound to (crate-internal, test
    /// support for constructing owner-bound ids directly).
    #[cfg(test)]
    #[inline]
    pub(crate) fn id(&self) -> u64 {
        self.id
    }

    /// Return the rule index for `rule_id`, `None` when it is out of range or
    /// minted by a different ruleset (a foreign id is a misuse the accessors
    /// reject instead of silently aliasing an unrelated rule).
    #[inline]
    fn checked_index(&self, rule_id: RuleId) -> Option<usize> {
        if rule_id.owner == self.id {
            let index = rule_id.index();
            (index < self.rules.len()).then_some(index)
        } else {
            None
        }
    }

    /// Map an application-supplied string id to its numeric [`RuleId`].
    pub fn rule_id(&self, string_id: &str) -> Option<RuleId> {
        self.ids.get(string_id).copied()
    }

    /// Map a numeric [`RuleId`] back to the application-supplied string id.
    ///
    /// Returns `None` for an out-of-range or foreign `rule_id` rather than
    /// panicking — a `RuleId` minted by another ruleset is rejected as a
    /// misuse (rule ids are only ever valid for the ruleset that minted them).
    pub fn string_id(&self, rule_id: RuleId) -> Option<&str> {
        self.checked_index(rule_id).map(|index| self.rules[index].id.as_str())
    }

    /// The geometry of a rule by [`RuleId`], or `None` for an out-of-range or
    /// foreign id.
    pub fn geometry(&self, rule_id: RuleId) -> Option<&Geometry<f64>> {
        self.checked_index(rule_id).map(|index| &self.rules[index].geometry)
    }

    /// The properties of a rule by [`RuleId`], or `None` for an out-of-range or
    /// foreign id.
    pub fn properties(&self, rule_id: RuleId) -> Option<&BTreeMap<String, PropertyValue>> {
        self.checked_index(rule_id).map(|index| &self.rules[index].properties)
    }

    /// The top-level precedence of a rule by [`RuleId`] (ADR-0015), or `None`
    /// for an out-of-range or foreign id. Missing at ingestion means `0`
    /// (unprioritized rules sort below any explicit priority).
    pub fn priority(&self, rule_id: RuleId) -> Option<i64> {
        self.checked_index(rule_id).map(|index| self.priorities[index])
    }

    /// The precomputed envelope of a rule by [`RuleId`], or `None` for an
    /// out-of-range or foreign id.
    pub fn envelope(&self, rule_id: RuleId) -> Option<&Rect<f64>> {
        self.checked_index(rule_id).map(|index| &self.envelopes[index])
    }

    /// The geometry of a rule by [`RuleId`] on the query hot path.
    ///
    /// The hot path only ever passes ids minted by this ruleset (they come from
    /// its own spatial/property indexes), so the owner check is skipped; an
    /// out-of-range id would be a logic bug and panics loudly. Prefer the
    /// checked [`Ruleset::geometry`] at public boundaries.
    #[inline]
    pub(crate) fn geometry_checked(&self, rule_id: RuleId) -> &Geometry<f64> {
        &self.rules[rule_id.index()].geometry
    }

    /// The properties of a rule by [`RuleId`] on the query hot path (see
    /// [`Ruleset::geometry_checked`]).
    #[inline]
    pub(crate) fn properties_checked(&self, rule_id: RuleId) -> &BTreeMap<String, PropertyValue> {
        &self.rules[rule_id.index()].properties
    }

    /// The precedence of a rule by [`RuleId`] on the query hot path (see
    /// [`Ruleset::geometry_checked`]).
    #[inline]
    pub(crate) fn priority_checked(&self, rule_id: RuleId) -> i64 {
        self.priorities[rule_id.index()]
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
        let value: serde_json::Value = serde_json::from_slice(input).map_err(|e| {
            SpatialError::invalid_geojson(format!("failed to parse canonical ruleset: {e}"))
        })?;
        let rules = value.as_array().ok_or_else(|| {
            SpatialError::invalid_geojson("failed to parse canonical ruleset: expected an array of rules")
        })?;
        // A present-but-wrong-typed `priority` must fail build with
        // `SR_RULESET_CONSTRUCTION_FAILED` naming the rule — the same gate as
        // GeoJSON ingestion (ADR-0015) — rather than surface as a generic
        // parse error. (A valid-but-negative integer flows through to the
        // build-time non-negativity gate below.)
        for rule_value in rules {
            if let Some(priority) = rule_value.get("priority") {
                let id = rule_value
                    .get("id")
                    .and_then(|id| id.as_str())
                    .unwrap_or("<unknown>");
                crate::ingestion::validate_priority(id, priority)?;
            }
        }
        let rules: Vec<Rule> = serde_json::from_value(value).map_err(|e| {
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
            owner: self.id,
        }
    }

    /// The prepared rule geometries for this ruleset (ADR-0010). A **dense**
    /// handle indexed by opaque [`RuleId`] — `len()` is the rule count, `get`
    /// is valid for any id, `iter` walks ruleset order — so callers (the
    /// benchmark ladder) never reconstruct a positional id-to-index map
    /// (architecture-hardening 04). Cloning the handle is cheap (`Rc`).
    ///
    /// This is the eager seam: calling it force-prepares every rule. The
    /// query path instead prepares lazily, per rule on first touch
    /// (memory-benchmark ticket 02).
    pub fn prepared(&self) -> PreparedRuleGeometries {
        PreparedRuleGeometries {
            inner: PreparedMemo::for_ruleset(&self.rules, self.id).snapshot_all(self.id),
            owner: self.id,
        }
    }

    /// Evaluate a batch of candidates against `query`, returning one outcome
    /// per candidate in input order (ADR-0004). Invalid candidates produce an
    /// [`CandidateOutcome::Invalid`] outcome without failing the batch
    /// (ADR-0005).
    ///
    /// Rule geometries are prepared lazily (ADR-0010, memory-benchmark 02):
    /// the per-thread memo fills per rule on first touch, so serving memory
    /// stays proportional to the rules candidates actually relate against.
    /// The relate loop checks a predicted `None` slot per touched rule and
    /// defers the few first-touch unprepared ones (see `evaluate.rs` — a
    /// batch-level pre-pass was measured and rejected for regressing
    /// sparse-touch throughput).
    pub fn query(&self, candidates: &[Candidate], query: &Query) -> Vec<CandidateOutcome> {
        self.prepare(query).evaluate_all(candidates)
    }

    /// Evaluate a batch and return the compact mask (`0` no match, `1`
    /// matched, `2` invalid), without materialising per-match rule ids
    /// (ADR-0004). Preparation is lazy per rule on first touch, as in
    /// [`Ruleset::query`].
    pub fn query_mask(&self, candidates: &[Candidate], query: &Query) -> Vec<u8> {
        self.prepare(query).evaluate_mask_all(candidates)
    }

    /// Resolve a batch of candidates against `query`, returning one
    /// [`ResolutionOutcome`] per candidate in input order (ADR-0015): the
    /// ordered applicable set, the winner, and first-provider-wins derived
    /// values. The match path and its mask are untouched; resolution is the
    /// same pipeline plus a precedence-ordered layer over the applicable set.
    pub fn resolve(&self, candidates: &[Candidate], query: &Query) -> Vec<ResolutionOutcome> {
        self.prepare(query).evaluate_resolve_all(candidates)
    }

    /// Resolve a batch and return the compact mask (`0` no resolution, `1`
    /// resolved, `2` invalid) without materialising the winner, values, or
    /// explanation (ADR-0015). Preparation is lazy per rule on first touch, as
    /// in [`Ruleset::resolve`].
    pub fn resolve_mask(&self, candidates: &[Candidate], query: &Query) -> Vec<u8> {
        self.prepare(query).evaluate_resolve_mask_all(candidates)
    }

    /// Compile a query into a reusable [`PreparedQuery`] holding the planning:
    /// excluded ids, this thread's lazy prepared-geometry memo (populated on
    /// first touch, ADR-0010), and the indexable `where` set.
    /// [`PreparedQuery::evaluate`] and [`PreparedQuery::evaluate_mask`] share
    /// this one preparation across the whole candidate batch. This is the
    /// planner hook ADR-0003 reserves — a cost-based planner would return a
    /// differently-shaped query here.
    pub fn prepare<'a>(&'a self, query: &Query) -> PreparedQuery<'a> {
        let excluded: HashSet<RuleId> = query
            .exclude_rule_ids
            .iter()
            .filter_map(|id| self.rule_id(id))
            .collect();
        let memo = PreparedMemo::for_ruleset(&self.rules, self.id);
        let where_filter = query
            .where_clause
            .as_ref()
            .and_then(|where_clause| self.property_index.indexable_matches(where_clause));
        PreparedQuery::new(self as &dyn crate::access::RuleAccess, query, excluded, memo, where_filter)
    }
}

/// Iteration over rules in ruleset order: id, geometry, and precomputed
/// envelope per rule. The seam the benchmark ladder consumes (ADR-0002) — it
/// replaces raw positional accessors, so the ruleset stops advertising its
/// storage layout.
pub struct RuleSource<'a> {
    rules: &'a [Rule],
    envelopes: &'a [Rect<f64>],
    owner: u64,
}

impl crate::access::RuleAccess for Ruleset {
    #[inline]
    fn query_envelope_into(&self, envelope: &Rect<f64>, out: &mut Vec<RuleId>) {
        self.query_envelope_into(envelope, out)
    }

    #[inline]
    fn geometry(&self, rule_id: RuleId) -> &Geometry<f64> {
        self.geometry_checked(rule_id)
    }

    #[inline]
    fn properties(&self, rule_id: RuleId) -> &BTreeMap<String, PropertyValue> {
        self.properties_checked(rule_id)
    }

    #[inline]
    fn priority(&self, rule_id: RuleId) -> i64 {
        self.priority_checked(rule_id)
    }
}

impl<'a> RuleSource<'a> {
    /// Iterate over `(id, geometry, envelope)` in ruleset order.
    pub fn iter(&self) -> impl Iterator<Item = (RuleId, &'a Geometry<f64>, &'a Rect<f64>)> {
        let rules = self.rules;
        let envelopes = self.envelopes;
        let owner = self.owner;
        rules
            .iter()
            .enumerate()
            .map(move |(index, rule)| {
                (
                    RuleId::new(index as u32, owner),
                    &rule.geometry,
                    &envelopes[index],
                )
            })
    }
}

/// A handle to one ruleset's prepared rule geometries (ADR-0010), indexed by
/// opaque [`RuleId`]. Callers fetch a rule's prepared form by id without ever
/// reading the numeric position (architecture-hardening 04).
pub struct PreparedRuleGeometries {
    inner: PreparedGeometries,
    owner: u64,
}

impl PreparedRuleGeometries {
    /// The prepared DE-9IM geometry for a rule by opaque [`RuleId`], or `None`
    /// for an out-of-range or foreign id.
    pub fn get(&self, rule_id: RuleId) -> Option<&PreparedGeometry<'static, Geometry<f64>>> {
        if rule_id.owner == self.owner {
            let index = rule_id.index();
            (index < self.inner.len()).then(|| &self.inner[index])
        } else {
            None
        }
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
    use crate::candidate::Candidate;
    use crate::prepared_cache;
    use geo::LineString;

    fn sample_rules() -> Vec<Rule> {
        vec![Rule {
            id: "zone".to_string(),
            properties: BTreeMap::new(),
            geometry: Geometry::Polygon(geo::Polygon::new(
                LineString::from(vec![(0.0, 0.0), (0.0, 1.0), (1.0, 1.0), (1.0, 0.0), (0.0, 0.0)]),
                vec![],
            )),
            priority: 0,
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

    #[test]
    fn build_rejects_non_finite_property_floats() {
        let mut rule = sample_rules().remove(0);
        rule.properties
            .insert("score".to_string(), crate::properties::PropertyValue::Float(f64::NAN));
        let err = Ruleset::build(vec![rule]).unwrap_err();
        assert_eq!(err.code, ErrorCode::RulesetConstructionFailed);
        assert!(err.message.contains("non-finite"));

        // A finite float is accepted.
        let mut rule = sample_rules().remove(0);
        rule.properties
            .insert("score".to_string(), crate::properties::PropertyValue::Float(4.2));
        assert!(Ruleset::build(vec![rule]).is_ok());
    }

    fn far_apart_rules() -> Vec<Rule> {
        let square = |x: f64, y: f64| {
            Geometry::Polygon(geo::Polygon::new(
                LineString::from(vec![
                    (x, y),
                    (x, y + 1.0),
                    (x + 1.0, y + 1.0),
                    (x + 1.0, y),
                    (x, y),
                ]),
                vec![],
            ))
        };
        vec![
            Rule {
                id: "zone-a".to_string(),
                properties: BTreeMap::new(),
                geometry: square(0.0, 0.0),
                priority: 0,
            },
            Rule {
                id: "zone-b".to_string(),
                properties: BTreeMap::new(),
                geometry: square(100.0, 100.0),
                priority: 0,
            },
        ]
    }

    fn candidate_at(x: f64, y: f64) -> Candidate {
        Candidate::new(
            "c".to_string(),
            Geometry::Polygon(geo::Polygon::new(
                LineString::from(vec![
                    (x - 0.5, y - 0.5),
                    (x - 0.5, y + 0.5),
                    (x + 0.5, y + 0.5),
                    (x + 0.5, y - 0.5),
                    (x - 0.5, y - 0.5),
                ]),
                vec![],
            )),
        )
    }

    /// Pins the lazy semantics (memory-benchmark ticket 02): a query whose
    /// candidates touch a subset of the rules prepares only that subset.
    #[test]
    fn query_prepares_only_the_touched_rules() {
        let ruleset = Ruleset::build(far_apart_rules()).unwrap();
        let candidate = candidate_at(0.5, 0.5); // touches zone-a only
        let query = Query::new(crate::query::SpatialPredicate::Intersects);

        let outcomes = ruleset.query(std::slice::from_ref(&candidate), &query);
        assert!(matches!(
            &outcomes[0],
            CandidateOutcome::Matched { rule_ids, .. }
                if rule_ids == &vec![RuleId::new(0, ruleset.id)]
        ));

        assert!(prepared_cache::slot_is_prepared(ruleset.id, 0));
        assert!(!prepared_cache::slot_is_prepared(ruleset.id, 1));
    }

    /// Rule ids in a result must keep the eager path's deterministic envelope
    /// (ascending) order even when the per-thread memo is only partially warm
    /// (memory-benchmark ticket 02): a previously-prepared rule must not jump
    /// ahead of a rule being prepared on first touch.
    #[test]
    fn rule_ids_stay_in_envelope_order_with_a_partially_warm_memo() {
        let ruleset = Ruleset::build(far_apart_rules()).unwrap();
        let query = Query::new(crate::query::SpatialPredicate::Intersects);

        // First query touches zone-b only, preparing it in this thread's memo.
        let b_only = candidate_at(100.5, 100.5);
        let _ = ruleset.query(std::slice::from_ref(&b_only), &query);

        // Second query touches both: zone-b is already prepared, zone-a is
        // not — the relate loop would record zone-b first without a re-sort.
        let both = Candidate::new(
            "c".to_string(),
            Geometry::Polygon(geo::Polygon::new(
                LineString::from(vec![
                    (-1.0, -1.0),
                    (-1.0, 101.0),
                    (101.0, 101.0),
                    (101.0, -1.0),
                    (-1.0, -1.0),
                ]),
                vec![],
            )),
        );
        let outcomes = ruleset.query(std::slice::from_ref(&both), &query);
        let CandidateOutcome::Matched { rule_ids, .. } = &outcomes[0] else {
            panic!("expected a match");
        };
        assert_eq!(
            rule_ids,
            &vec![RuleId::new(0, ruleset.id), RuleId::new(1, ruleset.id)]
        );
    }

    /// Worst case is unchanged (memory-benchmark ticket 02): a workload whose
    /// candidates touch every rule prepares everything.
    #[test]
    fn query_touching_every_rule_prepares_all_rules() {
        let mut rules = far_apart_rules();
        rules.push(Rule {
            id: "zone-c".to_string(),
            properties: BTreeMap::new(),
            geometry: Geometry::Polygon(geo::Polygon::new(
                LineString::from(vec![(-50.0, -50.0), (-50.0, 150.0), (150.0, 150.0), (150.0, -50.0), (-50.0, -50.0)]),
                vec![],
            )),
            priority: 0,
        });
        let ruleset = Ruleset::build(rules).unwrap();
        // A candidate whose envelope intersects all three rules.
        let candidate = Candidate::new(
            "c".to_string(),
            Geometry::Polygon(geo::Polygon::new(
                LineString::from(vec![
                    (-10.0, -10.0),
                    (-10.0, 120.0),
                    (120.0, 120.0),
                    (120.0, -10.0),
                    (-10.0, -10.0),
                ]),
                vec![],
            )),
        );
        let query = Query::new(crate::query::SpatialPredicate::Intersects);

        let outcomes = ruleset.query(std::slice::from_ref(&candidate), &query);
        assert_eq!(outcomes.len(), 1);

        for index in 0..3 {
            assert!(
                prepared_cache::slot_is_prepared(ruleset.id, index),
                "rule {index} must be prepared after a touch-all query"
            );
        }
    }

    /// The eager seam keeps its dense contract and force-prepares everything
    /// even when no query has touched anything yet.
    #[test]
    fn prepared_seam_force_prepares_every_rule() {
        let ruleset = Ruleset::build(far_apart_rules()).unwrap();

        let prepared = ruleset.prepared();
        assert_eq!(prepared.len(), 2);
        assert!(prepared
            .get(RuleId::new(0, ruleset.id))
            .expect("rule id minted by this ruleset")
            .bounding_rect()
            .is_some());
        assert!(prepared.iter().count() == 2);
        assert!(prepared_cache::slot_is_prepared(ruleset.id, 0));
        assert!(prepared_cache::slot_is_prepared(ruleset.id, 1));
    }
}
