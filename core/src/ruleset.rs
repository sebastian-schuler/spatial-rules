//! Immutable `Ruleset` compilation and the batch query engine
//! (ADR-0001/0002/0003/0004/0005).

use std::collections::{BTreeMap, HashMap, HashSet};

use geo::{BoundingRect, Geometry, Rect, Relate, Validation};

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

/// Answer a spatial predicate between a candidate and a rule via DE-9IM
/// (ADR-0008). `contains`/`within` are directional: `candidate` relates to
/// `rule`.
fn spatial_predicate_holds(
    predicate: SpatialPredicate,
    candidate: &Geometry<f64>,
    rule: &Geometry<f64>,
) -> bool {
    let matrix = candidate.relate(rule);
    match predicate {
        SpatialPredicate::Intersects => matrix.is_intersects(),
        SpatialPredicate::Contains => matrix.is_contains(),
        SpatialPredicate::Within => matrix.is_within(),
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

    /// The compile-time property equality/`$in` index.
    pub fn property_index(&self) -> &PropertyIndex {
        &self.property_index
    }

    /// Evaluate a batch of candidates against `query`, returning one outcome
    /// per candidate in input order (ADR-0004). Invalid candidates produce an
    /// [`CandidateOutcome::Invalid`] outcome without failing the batch
    /// (ADR-0005).
    pub fn query(&self, candidates: &[Candidate], query: &Query) -> Vec<CandidateOutcome> {
        let excluded: HashSet<RuleId> = query
            .exclude_rule_ids
            .iter()
            .filter_map(|id| self.rule_id(id))
            .collect();
        candidates
            .iter()
            .map(|candidate| self.evaluate_candidate(candidate, query, &excluded))
            .collect()
    }

    fn evaluate_candidate(
        &self,
        candidate: &Candidate,
        query: &Query,
        excluded: &HashSet<RuleId>,
    ) -> CandidateOutcome {
        // Candidate-level gate: unsupported type or invalid geometry yields an
        // `Invalid` outcome (never a batch failure, ADR-0005).
        if let Err(e) = ensure_supported_geometry(&candidate.geometry) {
            return CandidateOutcome::Invalid { reason: e.message };
        }
        if !candidate.geometry.is_valid() {
            return CandidateOutcome::Invalid {
                reason: format!("invalid geometry: {:?}", candidate.geometry.validation_errors()),
            };
        }
        let Some(bbox) = candidate.geometry.bounding_rect() else {
            return CandidateOutcome::Invalid {
                reason: "geometry has no bounding rectangle".to_string(),
            };
        };

        // Fixed pipeline: spatial bbox filter -> property predicate -> exact
        // DE-9IM relate (§15). Prepared geometries are a later ladder decision
        // (E/F, research 03); plain `Relate` is used here for correctness.
        let mut matched: Vec<RuleId> = Vec::new();
        for rule_id in self.query_envelope(&bbox) {
            if excluded.contains(&rule_id) {
                continue;
            }
            let rule = &self.rules[rule_id.0 as usize];
            if let Some(where_clause) = &query.where_clause {
                if !where_clause.eval(&rule.properties) {
                    continue;
                }
            }
            if spatial_predicate_holds(query.spatial, &candidate.geometry, &rule.geometry) {
                matched.push(rule_id);
            }
        }

        if matched.is_empty() {
            CandidateOutcome::NotMatched
        } else {
            CandidateOutcome::Matched { rule_ids: matched }
        }
    }
}
