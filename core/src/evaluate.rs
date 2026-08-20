//! Compiled-query evaluation and spatial predicate helpers.

use std::cell::RefCell;
use std::collections::HashSet;

use geo::{BooleanOps, GeodesicArea, Geometry, Relate};

use crate::candidate::{Candidate, CandidateClass};
use crate::prepared_cache::PreparedGeometries;
use crate::query::{CandidateOutcome, OverlapMetric, Query, SpatialPredicate};
use crate::rule::RuleId;
use crate::ruleset::Ruleset;
use crate::where_expr::WhereExpr;

/// Answer a spatial predicate from a DE-9IM matrix between a candidate and a
/// rule (ADR-0008, ADR-0012). `contains`/`within`/`covers`/`covered_by` are
/// directional: the matrix is `candidate` relates to `rule`.
fn spatial_predicate_holds(
    predicate: SpatialPredicate,
    matrix: geo::algorithm::relate::IntersectionMatrix,
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
    prepared: PreparedGeometries,
    where_filter: Option<HashSet<RuleId>>,
    include_overlap: bool,
    /// Reused across the batch so the spatial-index result is filled into one
    /// buffer instead of allocated per candidate.
    scratch: RefCell<Vec<RuleId>>,
}

impl<'a> PreparedQuery<'a> {
    pub(crate) fn new(
        ruleset: &'a Ruleset,
        query: &Query,
        excluded: HashSet<RuleId>,
        prepared: PreparedGeometries,
        where_filter: Option<HashSet<RuleId>>,
    ) -> Self {
        PreparedQuery {
            ruleset,
            spatial: query.spatial,
            where_clause: query.where_clause.clone(),
            excluded,
            prepared,
            where_filter,
            include_overlap: query.include_overlap,
            scratch: RefCell::new(Vec::new()),
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
        let mut scratch = self.scratch.borrow_mut();
        self.ruleset.query_envelope_into(&bbox, &mut scratch);
        for &rule_id in scratch.iter() {
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
                    if compute_overlaps {
                        overlaps.push(overlap_metric(
                            &candidate.geometry,
                            self.ruleset.geometry(rule_id),
                        ));
                    }
                }
            }
        }
        drop(scratch);

        EvalResult {
            matched: any_match,
            invalid: None,
            rule_ids: matched,
            overlaps: if compute_overlaps { Some(overlaps) } else { None },
        }
    }
}
