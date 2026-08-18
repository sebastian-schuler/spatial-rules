//! Immutable `Ruleset` compilation and the batch query engine
//! (ADR-0001/0002/0003/0004/0005).

use std::cell::RefCell;
use std::collections::{BTreeMap, HashMap, HashSet};
use std::rc::Rc;
use std::sync::atomic::{AtomicU64, Ordering};

use geo::{BoundingRect, Geometry, PreparedGeometry, Rect, Relate};

use crate::candidate::Candidate;
use crate::error::{ErrorCode, SpatialError};
use crate::property_index::{EqualityIndex, PropertyIndex};
use crate::properties::PropertyValue;
use crate::query::{CandidateOutcome, Query, SpatialPredicate};
use crate::rule::{Rule, RuleId};
use crate::spatial_index::{build_spatial_index, SpatialIndex, SpatialIndexKind};
use crate::validation::{classify_candidate, validate_rule_geometry};
use crate::where_expr::WhereExpr;

/// Owned prepared geometries for one ruleset, shared per thread via `Rc`
/// (`PreparedGeometry` is `!Send`; see ADR-0010).
type PreparedGeometries = Rc<Vec<PreparedGeometry<'static, Geometry<f64>>>>;

/// Assigns each [`Ruleset`] a unique identity, used as the per-thread
/// prepared-geometry cache key (ADR-0010).
static NEXT_RULESET_ID: AtomicU64 = AtomicU64::new(1);

// Per-thread cache of owned prepared geometries for the most recent ruleset.
// `PreparedGeometry` is `!Send` in geo 0.33, so it is cached per thread (as
// owned clones, prepared once per ruleset) rather than in the shared
// `Arc<Ruleset>`.
thread_local! {
    static PREPARED_CACHE: RefCell<Option<(u64, PreparedGeometries)>> = const { RefCell::new(None) };
}

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
        let property_index: Box<dyn PropertyIndex> = Box::new(EqualityIndex::build(&rules));

        Ok(Ruleset {
            id: NEXT_RULESET_ID.fetch_add(1, Ordering::Relaxed),
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
        let prepared = self.cached_prepared();
        let where_filter = query
            .where_clause
            .as_ref()
            .and_then(|where_clause| self.property_index.indexable_matches(where_clause));
        PreparedQuery {
            ruleset: self,
            spatial: query.spatial,
            where_clause: query.where_clause.clone(),
            excluded,
            prepared,
            where_filter,
        }
    }

    /// The rule geometries prepared for DE-9IM, cached per thread per ruleset
    /// (ADR-0010). `PreparedGeometry` is `!Send` in geo 0.33, so the owned form
    /// (cloned once per ruleset) lives in a thread-local keyed by ruleset
    /// identity rather than in the shared `Arc<Ruleset>`.
    fn cached_prepared(&self) -> PreparedGeometries {
        PREPARED_CACHE.with(|cache| {
            {
                let cached = cache.borrow();
                if let Some((id, prepared)) = cached.as_ref() {
                    if *id == self.id {
                        return prepared.clone();
                    }
                }
            }
            // Cache miss or stale entry: clone the rule geometries once per
            // thread per ruleset, prepare them (owned), and cache for reuse.
            let prepared: PreparedGeometries = Rc::new(
                self.rules
                    .iter()
                    .map(|rule| PreparedGeometry::from(rule.geometry.clone()))
                    .collect(),
            );
            *cache.borrow_mut() = Some((self.id, prepared.clone()));
            prepared
        })
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

/// A query compiled against a ruleset: the preparation that both `query` and
/// `query_mask` share (ADR-0003/0004). Owns the excluded ids, the prepared
/// rule geometries (cached per thread, ADR-0010), and the indexable `where`
/// set; evaluates each candidate through the fixed pipeline
/// (bbox → property → DE-9IM).
pub struct PreparedQuery<'a> {
    ruleset: &'a Ruleset,
    spatial: SpatialPredicate,
    where_clause: Option<WhereExpr>,
    excluded: HashSet<RuleId>,
    prepared: PreparedGeometries,
    where_filter: Option<HashSet<RuleId>>,
}

impl<'a> PreparedQuery<'a> {
    /// Evaluate one candidate, collecting matching rule ids (ADR-0004).
    pub fn evaluate(&self, candidate: &Candidate) -> CandidateOutcome {
        match self.evaluate_verdict(candidate, true) {
            (Verdict::Matched, matched) => CandidateOutcome::Matched { rule_ids: matched },
            (Verdict::NotMatched, _) => CandidateOutcome::NotMatched,
            (Verdict::Invalid { reason }, _) => CandidateOutcome::Invalid { reason },
        }
    }

    /// Evaluate one candidate to the compact `0/1/2` mask (ADR-0004), without
    /// materialising matching rule ids.
    pub fn evaluate_mask(&self, candidate: &Candidate) -> u8 {
        match self.evaluate_verdict(candidate, false).0 {
            Verdict::Matched => 1,
            Verdict::NotMatched => 0,
            Verdict::Invalid { .. } => 2,
        }
    }

    fn evaluate_verdict(&self, candidate: &Candidate, collect_ids: bool) -> (Verdict, Vec<RuleId>) {
        // Candidate-level gate (ADR-0005): unsupported type, invalid geometry,
        // or missing bbox yields an `Invalid` outcome, never a batch failure.
        let bbox = match classify_candidate(&candidate.geometry) {
            Ok(bbox) => bbox,
            Err(reason) => return (Verdict::Invalid { reason }, Vec::new()),
        };

        // Fixed pipeline: spatial bbox filter -> property predicate -> exact
        // DE-9IM relate against prepared rule geometries (§15, research 03).
        // A compile-time equality/`$in` index answers the property step when
        // the clause is indexable (ADR-0003); otherwise fall back to eval.
        let mut matched: Vec<RuleId> = Vec::new();
        let mut any_match = false;
        for rule_id in self.ruleset.query_envelope(&bbox) {
            if self.excluded.contains(&rule_id) {
                continue;
            }
            match &self.where_filter {
                Some(filter) => {
                    if !filter.contains(&rule_id) {
                        continue;
                    }
                }
                None => {
                    if let Some(where_clause) = &self.where_clause {
                        if !where_clause.eval(self.ruleset.properties(rule_id)) {
                            continue;
                        }
                    }
                }
            }
            let matrix = candidate.geometry.relate(&self.prepared[rule_id.0 as usize]);
            if spatial_predicate_holds(self.spatial, matrix) {
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
