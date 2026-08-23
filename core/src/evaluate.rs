//! Compiled-query evaluation and spatial predicate helpers.

use std::cell::RefCell;
use std::collections::{BTreeMap, HashSet};

use geo::algorithm::relate::IntersectionMatrix;
use geo::{BooleanOps, GeodesicArea, Geometry, Rect, Relate};

use crate::candidate::{Candidate, CandidateClass};
use crate::prepared_cache::PreparedMemo;
use crate::properties::PropertyValue;
use crate::query::{ApplicableRule, CandidateOutcome, OverlapMetric, Query, ResolutionOutcome, SpatialPredicate};
use crate::rule::RuleId;
use crate::ruleset::Ruleset;
use crate::where_expr::WhereExpr;

/// Answer a spatial predicate from a DE-9IM matrix between a candidate and a
/// rule (ADR-0008, ADR-0012). `contains`/`within`/`covers`/`covered_by` are
/// directional: the matrix is `candidate` relates to `rule`.
fn spatial_predicate_holds(
    predicate: SpatialPredicate,
    matrix: &IntersectionMatrix,
) -> bool {
    match predicate {
        SpatialPredicate::Intersects => matrix.is_intersects(),
        SpatialPredicate::Contains => matrix.is_contains(),
        SpatialPredicate::Within => matrix.is_within(),
        SpatialPredicate::Covers => matrix.is_covers(),
        SpatialPredicate::CoveredBy => matrix.is_coveredby(),
        SpatialPredicate::Touches => matrix.is_touches(),
        SpatialPredicate::Overlaps => matrix.is_overlaps(),
    }
}

/// Geodesic overlap metrics for a matched candidate-to-rule pair (ADR-0012).
///
/// The intersection is computed with [`BooleanOps`] and measured with
/// [`GeodesicArea`] (spherical), so lon/lat is never treated as planar
/// (Initial-plan section 14). Polygon/MultiPolygon candidates are guaranteed valid by
/// the upstream gates; a Point/MultiPoint candidate has zero area, so its
/// overlap area and ratio are both `0` (filtering-scale ticket 01).
///
/// `geodesic_area_signed().abs()` is used (not `geodesic_area_unsigned`) so the
/// measure is robust to exterior-ring winding: `_unsigned` assumes a
/// counter-clockwise exterior per the Simple Features convention and reports
/// the Earth-complement area for a clockwise exterior, while the signed
/// magnitude is correct for any winding of a polygon smaller than half the
/// Earth (always true for rules/candidates).
fn overlap_metric(candidate: &Geometry<f64>, rule: &Geometry<f64>) -> OverlapMetric {
    // A point has zero area: there is no polygon intersection to measure.
    if matches!(candidate, Geometry::Point(_) | Geometry::MultiPoint(_)) {
        return OverlapMetric {
            overlap_area: 0.0,
            overlap_ratio: 0.0,
        };
    }
    let intersection = match (candidate, rule) {
        (Geometry::Polygon(c), Geometry::Polygon(r)) => c.intersection(r),
        (Geometry::Polygon(c), Geometry::MultiPolygon(r)) => c.intersection(r),
        (Geometry::MultiPolygon(c), Geometry::Polygon(r)) => c.intersection(r),
        (Geometry::MultiPolygon(c), Geometry::MultiPolygon(r)) => c.intersection(r),
        _ => unreachable!("overlap metrics require Polygon/MultiPolygon candidates and rules"),
    };
    let overlap_area = intersection.geodesic_area_signed().abs();
    let candidate_area = candidate.geodesic_area_signed().abs();
    let overlap_ratio = if candidate_area > 0.0 {
        overlap_area / candidate_area
    } else {
        0.0
    };
    OverlapMetric {
        overlap_area,
        overlap_ratio,
    }
}

struct EvalResult {
    matched: bool,
    invalid: Option<String>,
    rule_ids: Vec<RuleId>,
    overlaps: Option<Vec<OverlapMetric>>,
}

pub struct PreparedQuery<'a> {
    ruleset: &'a Ruleset,
    spatial: SpatialPredicate,
    where_clause: Option<WhereExpr>,
    excluded: HashSet<RuleId>,
    /// This thread's per-rule prepared-geometry memo for the ruleset
    /// (ADR-0010): slots fill lazily, on first touch (memory-benchmark 02).
    memo: PreparedMemo<'a>,
    where_filter: Option<HashSet<RuleId>>,
    include_overlap: bool,
    /// Reused across the batch so the spatial-index result is filled into one
    /// buffer instead of allocated per candidate; then filtered in place to
    /// the rules the candidate will relate against (envelope order).
    scratch: RefCell<Vec<RuleId>>,
}

impl<'a> PreparedQuery<'a> {
    pub(crate) fn new(
        ruleset: &'a Ruleset,
        query: &Query,
        excluded: HashSet<RuleId>,
        memo: PreparedMemo<'a>,
        where_filter: Option<HashSet<RuleId>>,
    ) -> Self {
        PreparedQuery {
            ruleset,
            spatial: query.spatial,
            where_clause: query.where_clause.clone(),
            excluded,
            memo,
            where_filter,
            include_overlap: query.include_overlap,
            scratch: RefCell::new(Vec::new()),
        }
    }

    /// Evaluate a whole batch with **per-rule lazy preparation** (ADR-0010,
    /// memory-benchmark ticket 02). The memo persists across the batch (and
    /// across batches, until `replace`), so a candidate only pays the missing
    /// slot check on rules the batch touches for the first time.
    ///
    /// A batch-level pre-pass that collects the union of touched ids up front
    /// was measured and rejected: it runs the envelope filter a second time per
    /// candidate, which regresses sparse-touch workloads (the memory-scale
    /// `queries_per_sec` cell) by ~30% because that workload is index-traversal
    /// bound. The per-candidate check is the ticket's sanctioned fallback.
    pub(crate) fn evaluate_all(&self, candidates: &[Candidate]) -> Vec<CandidateOutcome> {
        candidates.iter().map(|c| self.evaluate(c)).collect()
    }

    /// Batch form of [`PreparedQuery::evaluate_mask`] — same lazy-preparation
    /// contract as [`PreparedQuery::evaluate_all`].
    pub(crate) fn evaluate_mask_all(&self, candidates: &[Candidate]) -> Vec<u8> {
        candidates.iter().map(|c| self.evaluate_mask(c)).collect()
    }

    /// Batch form of [`PreparedQuery::evaluate_resolve`] — same lazy
    /// preparation as the match path.
    pub(crate) fn evaluate_resolve_all(&self, candidates: &[Candidate]) -> Vec<ResolutionOutcome> {
        candidates.iter().map(|c| self.evaluate_resolve(c)).collect()
    }

    /// Batch form of [`PreparedQuery::evaluate_resolve_mask`] — same lazy
    /// preparation as the match path.
    pub(crate) fn evaluate_resolve_mask_all(&self, candidates: &[Candidate]) -> Vec<u8> {
        candidates.iter().map(|c| self.evaluate_resolve_mask(c)).collect()
    }

    /// Whether a candidate-touching rule reaches the exact DE-9IM step:
    /// not excluded and admitted by the property filter (ADR-0003 pipeline).
    fn reaches_relate(&self, rule_id: RuleId) -> bool {
        if self.excluded.contains(&rule_id) {
            return false;
        }
        match &self.where_filter {
            Some(filter) => filter.contains(&rule_id),
            None => match &self.where_clause {
                Some(where_clause) => {
                    where_clause.eval(self.ruleset.properties(rule_id))
                }
                None => true,
            },
        }
    }

    pub fn evaluate(&self, candidate: &Candidate) -> CandidateOutcome {
        let result = self.evaluate_result(candidate, true);
        if let Some(reason) = result.invalid {
            CandidateOutcome::Invalid { reason }
        } else if result.matched {
            CandidateOutcome::Matched {
                rule_ids: result.rule_ids,
                overlaps: result.overlaps,
            }
        } else {
            CandidateOutcome::NotMatched
        }
    }

    pub fn evaluate_mask(&self, candidate: &Candidate) -> u8 {
        let result = self.evaluate_result(candidate, false);
        if result.invalid.is_some() {
            2
        } else if result.matched {
            1
        } else {
            0
        }
    }

    /// The shared relate step of the fixed pipeline (bbox → property → exact
    /// DE-9IM): fill the scratch buffer with the candidate-touching, admitted
    /// rules, lazily prepare them, and invoke `on_hold(rule_id, matrix)` for
    /// each in envelope order. Both the match and resolve paths layer their
    /// per-rule action on this one loop; the mask hot path is unchanged
    /// because the closure does the same per-rule work it always did.
    ///
    /// Lazy preparation (ADR-0010, memory-benchmark 02): filter the envelope
    /// results to the rules this candidate will relate against, prepare
    /// exactly the missing ones, then relate in envelope order — so matched
    /// rule ids stay in the eager path's deterministic ascending order whether
    /// or not the memo was already warm. Warm batches find everything
    /// prepared: `ensure` skips every slot fill and the loop adds one
    /// predicted `None` check per touched rule.
    fn relate_touched<F>(&self, candidate: &Candidate, bbox: &Rect<f64>, mut on_hold: F)
    where
        F: FnMut(RuleId, &IntersectionMatrix),
    {
        let mut scratch = self.scratch.borrow_mut();
        self.ruleset.query_envelope_into(bbox, &mut scratch);
        scratch.retain(|&rule_id| self.reaches_relate(rule_id));
        if !scratch.is_empty() {
            self.memo.ensure(&scratch);
            let slots = self.memo.slots();
            for &rule_id in scratch.iter() {
                let prepared = slots[rule_id.index()]
                    .as_ref()
                    .expect("touched rules are prepared");
                let matrix = candidate.geometry.relate(prepared);
                on_hold(rule_id, &matrix);
            }
        }
    }

    fn evaluate_result(&self, candidate: &Candidate, collect_ids: bool) -> EvalResult {
        // Candidate classification is computed at intake, so the hot path only
        // reads the cached envelope or returns the recorded invalid reason.
        let bbox = match candidate.class() {
            CandidateClass::Valid { envelope } => *envelope,
            CandidateClass::Invalid { reason } => {
                return EvalResult {
                    matched: false,
                    invalid: Some(reason.clone()),
                    rule_ids: Vec::new(),
                    overlaps: None,
                };
            }
        };

        // Fixed pipeline: bbox -> property -> exact DE-9IM.
        let compute_overlaps = collect_ids && self.include_overlap;
        let mut matched: Vec<RuleId> = Vec::new();
        let mut overlaps: Vec<OverlapMetric> = Vec::new();
        let mut any_match = false;
        self.relate_touched(candidate, &bbox, |rule_id, matrix| {
            if spatial_predicate_holds(self.spatial, matrix) {
                any_match = true;
                if collect_ids {
                    matched.push(rule_id);
                    if compute_overlaps {
                        overlaps.push(overlap_metric(
                            &candidate.geometry,
                            self.ruleset.geometry(rule_id),
                        ));
                    }
                }
            }
        });

        EvalResult {
            matched: any_match,
            invalid: None,
            rule_ids: matched,
            overlaps: if compute_overlaps { Some(overlaps) } else { None },
        }
    }

    /// The applicable rule ids for a candidate in envelope order (the fixed
    /// bbox → property → exact DE-9IM pipeline), or the recorded invalid
    /// reason. Shared by the full resolve path and the compact mask path so
    /// the relate loop lives in one body, as it does for the match path.
    fn applicable_ids(&self, candidate: &Candidate) -> Result<Vec<RuleId>, String> {
        let bbox = match candidate.class() {
            CandidateClass::Valid { envelope } => *envelope,
            CandidateClass::Invalid { reason } => return Err(reason.clone()),
        };
        let mut ids: Vec<RuleId> = Vec::new();
        self.relate_touched(candidate, &bbox, |rule_id, matrix| {
            if spatial_predicate_holds(self.spatial, matrix) {
                ids.push(rule_id);
            }
        });
        Ok(ids)
    }

    /// Resolve one candidate (ADR-0015): gather the **applicable** rules
    /// (spatial predicate holds + where clause admits + not excluded) — the
    /// same fixed bbox → property → exact DE-9IM pipeline as the match path —
    /// then order them by precedence (priority desc, ties by declaration
    /// order / ascending rule id). The winner is the head of that order; the
    /// values are a first-provider-wins merge of the applicable rules'
    /// properties down the order; the ordered set itself is the explanation.
    ///
    /// **Collect-then-resolve**: no early exit at the first spatial hit, the
    /// merge needs the full applicable set (ADR-0015 stance).
    pub fn evaluate_resolve(&self, candidate: &Candidate) -> ResolutionOutcome {
        let ids = match self.applicable_ids(candidate) {
            Ok(ids) => ids,
            Err(reason) => {
                return ResolutionOutcome::Invalid { reason };
            }
        };
        if ids.is_empty() {
            return ResolutionOutcome::NotMatched;
        }

        let mut applicable: Vec<ApplicableRule> = ids
            .into_iter()
            .map(|rule_id| ApplicableRule {
                rule_id,
                priority: self.ruleset.priority(rule_id),
                spatial_matched: true,
                property_matched: true,
            })
            .collect();

        // Precedence: priority desc, ties by declaration order (ascending rule
        // id). The explicit tie-break keeps the order deterministic even if
        // the envelope order ever changes.
        applicable.sort_by(|a, b| {
            b.priority
                .cmp(&a.priority)
                .then_with(|| a.rule_id.cmp(&b.rule_id))
        });

        let winner = applicable[0].rule_id;
        let mut values: BTreeMap<String, PropertyValue> = BTreeMap::new();
        for rule in &applicable {
            for (key, value) in self.ruleset.properties(rule.rule_id) {
                values.entry(key.clone()).or_insert_with(|| value.clone());
            }
        }

        ResolutionOutcome::Resolved {
            winner,
            values,
            applicable,
        }
    }

    /// The compact form of [`PreparedQuery::evaluate_resolve`]: `0` no
    /// resolution, `1` resolved, `2` invalid (ADR-0015). Mask-only callers pay
    /// for the collect-then-resolve relate loop but not for the winner sort,
    /// the values merge, or the explanation — mirroring how the match mask
    /// skips per-match rule ids (ADR-0004).
    pub fn evaluate_resolve_mask(&self, candidate: &Candidate) -> u8 {
        match self.applicable_ids(candidate) {
            Err(_) => 2,
            Ok(ids) if ids.is_empty() => 0,
            Ok(_) => 1,
        }
    }
}
