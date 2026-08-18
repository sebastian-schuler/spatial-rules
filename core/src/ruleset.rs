//! Immutable `Ruleset` compilation and the batch query engine
//! (ADR-0001/0002/0003/0004/0005).

use std::collections::{BTreeMap, HashMap, HashSet};

use geo::{BoundingRect, Geometry, PreparedGeometry, Rect, Relate, Validation};

use crate::candidate::Candidate;
use crate::error::{ErrorCode, SpatialError};
use crate::property_index::PropertyIndex;
use crate::properties::PropertyValue;
use crate::query::{CandidateOutcome, Query, SpatialPredicate};
use crate::rule::{Rule, RuleId};
use crate::spatial_index::{build_spatial_index, SpatialIndex, SpatialIndexKind};
use crate::validation::{ensure_supported_geometry, validate_rule_geometry};

/// An immutable, query-optimized collection of rules (CONTEXT.md §6).
///
/// Fully built before publication and never mutated afterwards; shared across
/// requests behind an `Arc`.
pub struct Ruleset {
    rules: Vec<Rule>,
    ids: HashMap<String, RuleId>,
    envelopes: Vec<Rect<f64>>,
    spatial_index: Box<dyn SpatialIndex>,
    property_index: PropertyIndex,
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

/// Answer a spatial predicate from a DE-9IM matrix between a candidate and a
/// rule (ADR-0008). `contains`/`within` are directional: the matrix is
/// `candidate` relates to `rule`.
fn spatial_predicate_holds(
    predicate: SpatialPredicate,
    matrix: geo::algorithm::relate::IntersectionMatrix,
) -> bool {
    match predicate {
        SpatialPredicate::Intersects => matrix.is_intersects(),
        SpatialPredicate::Contains => matrix.is_contains(),
        SpatialPredicate::Within => matrix.is_within(),
    }
}

/// A candidate verdict before the `Matched` ids are attached (ADR-0004).
/// `query` and `query_mask` share the same evaluation but differ in whether
/// they collect the matching rule ids.
enum Verdict {
    Matched,
    NotMatched,
    Invalid { reason: String },
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
        let property_index = PropertyIndex::build(&rules);

        Ok(Ruleset {
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

    /// All rule ids in ruleset order.
    pub fn rule_ids(&self) -> Vec<RuleId> {
        (0..self.rules.len())
            .map(|index| RuleId(index as u32))
            .collect()
    }

    /// Rule geometries in ruleset order (benchmark ladder, ADR-0002).
    pub fn rule_geometries(&self) -> Vec<&Geometry<f64>> {
        self.rules.iter().map(|rule| &rule.geometry).collect()
    }

    /// Evaluate a batch of candidates against `query`, returning one outcome
    /// per candidate in input order (ADR-0004). Invalid candidates produce an
    /// [`CandidateOutcome::Invalid`] outcome without failing the batch
    /// (ADR-0005).
    pub fn query(&self, candidates: &[Candidate], query: &Query) -> Vec<CandidateOutcome> {
        let (excluded, prepared, where_filter) = self.prepare_query(query);
        candidates
            .iter()
            .map(|candidate| {
                let (verdict, matched) = self.evaluate_candidate(
                    candidate,
                    query,
                    &excluded,
                    &prepared,
                    &where_filter,
                    true,
                );
                match verdict {
                    Verdict::Matched => CandidateOutcome::Matched { rule_ids: matched },
                    Verdict::NotMatched => CandidateOutcome::NotMatched,
                    Verdict::Invalid { reason } => CandidateOutcome::Invalid { reason },
                }
            })
            .collect()
    }

    /// Evaluate a batch and return the compact mask (`0` no match, `1`
    /// matched, `2` invalid), without materialising per-match rule ids
    /// (ADR-0004).
    pub fn query_mask(&self, candidates: &[Candidate], query: &Query) -> Vec<u8> {
        let (excluded, prepared, where_filter) = self.prepare_query(query);
        candidates
            .iter()
            .map(|candidate| {
                let (verdict, _) = self.evaluate_candidate(
                    candidate,
                    query,
                    &excluded,
                    &prepared,
                    &where_filter,
                    false,
                );
                match verdict {
                    Verdict::Matched => 1,
                    Verdict::NotMatched => 0,
                    Verdict::Invalid { .. } => 2,
                }
            })
            .collect()
    }

    fn prepare_query<'a>(
        &'a self,
        query: &Query,
    ) -> (
        HashSet<RuleId>,
        Vec<PreparedGeometry<'a, &'a Geometry<f64>>>,
        Option<HashSet<RuleId>>,
    ) {
        let excluded: HashSet<RuleId> = query
            .exclude_rule_ids
            .iter()
            .filter_map(|id| self.rule_id(id))
            .collect();
        // Per-call (per-worker) preparation (research 03): ~5 ms for 30 rules,
        // amortized over the batch. `PreparedGeometry` is not Send/Sync in geo
        // 0.33, so it stays local to this call.
        let prepared: Vec<_> = self
            .rules
            .iter()
            .map(|rule| PreparedGeometry::from(&rule.geometry))
            .collect();
        let where_filter = query
            .where_clause
            .as_ref()
            .and_then(|where_clause| self.property_index.indexable_matches(where_clause));
        (excluded, prepared, where_filter)
    }

    fn evaluate_candidate<'a>(
        &self,
        candidate: &Candidate,
        query: &Query,
        excluded: &HashSet<RuleId>,
        prepared: &[PreparedGeometry<'a, &'a Geometry<f64>>],
        where_filter: &Option<HashSet<RuleId>>,
        collect_ids: bool,
    ) -> (Verdict, Vec<RuleId>) {
        // Candidate-level gate: unsupported type or invalid geometry yields an
        // `Invalid` outcome (never a batch failure, ADR-0005).
        if let Err(e) = ensure_supported_geometry(&candidate.geometry) {
            return (Verdict::Invalid { reason: e.message }, Vec::new());
        }
        if !candidate.geometry.is_valid() {
            return (
                Verdict::Invalid {
                    reason: format!(
                        "invalid geometry: {:?}",
                        candidate.geometry.validation_errors()
                    ),
                },
                Vec::new(),
            );
        }
        let Some(bbox) = candidate.geometry.bounding_rect() else {
            return (
                Verdict::Invalid {
                    reason: "geometry has no bounding rectangle".to_string(),
                },
                Vec::new(),
            );
        };

        // Fixed pipeline: spatial bbox filter -> property predicate -> exact
        // DE-9IM relate against prepared rule geometries (§15, research 03).
        // A compile-time equality/`$in` index answers the property step when
        // the clause is indexable (ADR-0003); otherwise fall back to eval.
        let mut matched: Vec<RuleId> = Vec::new();
        let mut any_match = false;
        for rule_id in self.query_envelope(&bbox) {
            if excluded.contains(&rule_id) {
                continue;
            }
            match where_filter {
                Some(filter) => {
                    if !filter.contains(&rule_id) {
                        continue;
                    }
                }
                None => {
                    if let Some(where_clause) = &query.where_clause {
                        if !where_clause.eval(&self.rules[rule_id.0 as usize].properties) {
                            continue;
                        }
                    }
                }
            }
            let matrix = candidate.geometry.relate(&prepared[rule_id.0 as usize]);
            if spatial_predicate_holds(query.spatial, matrix) {
                any_match = true;
                if collect_ids {
                    matched.push(rule_id);
                }
            }
        }

        if any_match {
            (Verdict::Matched, matched)
        } else {
            (Verdict::NotMatched, matched)
        }
    }
}
