//! Query model and per-candidate outcomes (ADR-0004, ADR-0005, ADR-0015).

use std::collections::BTreeMap;
use std::str::FromStr;

use crate::runtime::aggregate::{Aggregate, AggregateSpec};
use crate::error::SpatialError;
use crate::model::properties::PropertyValue;
use crate::model::rule::RuleId;
use crate::model::temporal::TemporalInstant;
use crate::model::where_expr::WhereExpr;

/// A spatial predicate between a candidate and a rule (ADR-0008, ADR-0012).
///
/// `WithinDistance` is a metric predicate (ADR-0016), not DE-9IM: it is
/// admitted when the candidate is within `Query.distance_meters` of the rule.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SpatialPredicate {
    Intersects,
    Contains,
    Within,
    Covers,
    CoveredBy,
    Touches,
    Overlaps,
    WithinDistance,
}

impl SpatialPredicate {
    pub fn as_str(self) -> &'static str {
        match self {
            SpatialPredicate::Intersects => "intersects",
            SpatialPredicate::Contains => "contains",
            SpatialPredicate::Within => "within",
            SpatialPredicate::Covers => "covers",
            SpatialPredicate::CoveredBy => "covered_by",
            SpatialPredicate::Touches => "touches",
            SpatialPredicate::Overlaps => "overlaps",
            SpatialPredicate::WithinDistance => "withinDistance",
        }
    }
}

impl std::str::FromStr for SpatialPredicate {
    type Err = SpatialError;

    /// Parse the `spatial.predicate` string; anything outside the supported
    /// set is `SR_UNSUPPORTED_SPATIAL_PREDICATE`.
    fn from_str(value: &str) -> Result<Self, SpatialError> {
        match value {
            "intersects" => Ok(SpatialPredicate::Intersects),
            "contains" => Ok(SpatialPredicate::Contains),
            "within" => Ok(SpatialPredicate::Within),
            "covers" => Ok(SpatialPredicate::Covers),
            "covered_by" => Ok(SpatialPredicate::CoveredBy),
            "touches" => Ok(SpatialPredicate::Touches),
            "overlaps" => Ok(SpatialPredicate::Overlaps),
            "withinDistance" => Ok(SpatialPredicate::WithinDistance),
            other => Err(SpatialError::unsupported_spatial_predicate(format!(
                "unsupported spatial predicate: {other}"
            ))),
        }
    }
}

/// One batch evaluation: a spatial predicate, an optional property `where`
/// clause, optional excluded rule ids (CONTEXT.md §5), an opt-in overlap
/// computation (ADR-0012), a distance radius for `withinDistance` (ADR-0016),
/// and an optional reference time for temporal predicates (ADR-0017).
#[derive(Debug, Clone, PartialEq)]
pub struct Query {
    pub spatial: SpatialPredicate,
    pub where_clause: Option<WhereExpr>,
    pub exclude_rule_ids: Vec<String>,
    /// When true, the rich path computes per-matched-rule geodesic overlap
    /// area/ratio (ADR-0012). The hot-path mask ignores this flag.
    pub include_overlap: bool,
    /// The `withinDistance` radius in meters (ADR-0016); `Some` only when
    /// `spatial == WithinDistance`.
    pub distance_meters: Option<f64>,
    /// The reference time for `$activeAt` predicates (ADR-0017). Required
    /// whenever the `where` clause contains one.
    pub at: Option<TemporalInstant>,
    /// Per-candidate analytics over the applicable rule set (ADR-0018),
    /// computed on the rich path only.
    pub aggregate: Option<AggregateSpec>,
}

impl Query {
    pub fn new(spatial: SpatialPredicate) -> Self {
        Query {
            spatial,
            where_clause: None,
            exclude_rule_ids: Vec::new(),
            include_overlap: false,
            distance_meters: None,
            at: None,
            aggregate: None,
        }
    }

    pub fn with_where(mut self, where_clause: WhereExpr) -> Self {
        self.where_clause = Some(where_clause);
        self
    }

    pub fn with_exclusions(mut self, exclude_rule_ids: Vec<String>) -> Self {
        self.exclude_rule_ids = exclude_rule_ids;
        self
    }

    pub fn with_overlap(mut self) -> Self {
        self.include_overlap = true;
        self
    }

    /// Set the `withinDistance` radius in meters (ADR-0016).
    pub fn with_distance(mut self, distance_meters: f64) -> Self {
        self.distance_meters = Some(distance_meters);
        self
    }

    /// Set the reference time for `$activeAt` predicates (ADR-0017).
    pub fn with_at(mut self, at: TemporalInstant) -> Self {
        self.at = Some(at);
        self
    }

    /// Request per-candidate aggregates over the applicable rule set (ADR-0018).
    pub fn with_aggregate(mut self, aggregate: AggregateSpec) -> Self {
        self.aggregate = Some(aggregate);
        self
    }

    /// Validate the invariants the JSON shape enforces so a programmatic
    /// [`Query`] built with the public builders cannot silently misbehave
    /// (the same rules [`Query::from_json`] applies):
    ///
    /// - `distance_meters` is `Some` (finite, positive, and only ever for
    ///   `WithinDistance`).
    /// - `at` is present whenever the `where` clause uses `$activeAt`.
    ///
    /// Returns the human-readable reason a malformed query would be rejected
    /// with, or `None` when the query is well-formed. Evaluation consumes this
    /// seam so a programmatic violation surfaces as a per-candidate
    /// [`CandidateOutcome::Invalid`] (or [`ResolutionOutcome::Invalid`]) rather
    /// than an unexplained non-match.
    pub fn validate(&self) -> Option<&'static str> {
        let distance_reason: Option<&'static str> = match self.spatial {
            SpatialPredicate::WithinDistance => {
                if self.distance_meters.is_some_and(|d| d.is_finite() && d > 0.0) {
                    None
                } else {
                    Some("withinDistance requires a positive distance")
                }
            }
            _ => {
                if self.distance_meters.is_some() {
                    Some("distance is only valid with the 'withinDistance' predicate")
                } else {
                    None
                }
            }
        };
        distance_reason.or_else(|| {
            if self.at.is_none() && self.where_clause.as_ref().is_some_and(WhereExpr::has_active_at) {
                Some("'at' is required when a '$activeAt' predicate is present")
            } else {
                None
            }
        })
        .or_else(|| self.aggregate.as_ref().and_then(AggregateSpec::validate))
    }

    /// Parse the JSON query shape (Initial-plan §22):
    /// `{ "spatial": { "predicate": "..." }, "where": {...}, "excludeRuleIds": [...], "includeOverlap": true, "at": "YYYY-MM-DDTHH:MM" }`.
    /// For `withinDistance` the `spatial` object also carries `"distance"` in
    /// meters (ADR-0016); `at` is required when the `where` clause uses
    /// `$activeAt` (ADR-0017).
    pub fn from_json(value: &serde_json::Value) -> Result<Self, SpatialError> {
        let object = value
            .as_object()
            .ok_or_else(|| SpatialError::invalid_query("query must be an object"))?;

        let spatial = object
            .get("spatial")
            .ok_or_else(|| SpatialError::invalid_query("missing 'spatial'"))?;
        let spatial_object = spatial
            .as_object()
            .ok_or_else(|| SpatialError::invalid_query("'spatial' must be an object"))?;
        let predicate = spatial_object
            .get("predicate")
            .and_then(|value| value.as_str())
            .ok_or_else(|| SpatialError::invalid_query("'spatial.predicate' must be a string"))?;
        let spatial = SpatialPredicate::from_str(predicate)?;

        let distance_meters = match spatial_object.get("distance") {
            None => None,
            Some(value) => {
                let distance = value.as_f64().ok_or_else(|| {
                    SpatialError::invalid_query("'spatial.distance' must be a number")
                })?;
                if !distance.is_finite() || distance <= 0.0 {
                    return Err(SpatialError::invalid_query(
                        "'spatial.distance' must be a finite positive number",
                    ));
                }
                Some(distance)
            }
        };
        if spatial == SpatialPredicate::WithinDistance && distance_meters.is_none() {
            return Err(SpatialError::invalid_query(
                "'withinDistance' requires a positive 'spatial.distance'",
            ));
        }
        if spatial != SpatialPredicate::WithinDistance && distance_meters.is_some() {
            return Err(SpatialError::invalid_query(
                "'spatial.distance' is only valid with the 'withinDistance' predicate",
            ));
        }

        let where_clause = match object.get("where") {
            None => None,
            Some(value) => Some(WhereExpr::parse(value)?),
        };

        let at = match object.get("at") {
            None => None,
            Some(value) => {
                let text = value.as_str().ok_or_else(|| {
                    SpatialError::invalid_query("'at' must be an ISO-8601 string")
                })?;
                let instant = TemporalInstant::parse_iso8601(text)?;
                Some(instant)
            }
        };
        if at.is_none() && where_clause.as_ref().is_some_and(WhereExpr::has_active_at) {
            return Err(SpatialError::invalid_query(
                "'at' is required when a '$activeAt' predicate is present",
            ));
        }

        let aggregate = match object.get("aggregate") {
            None => None,
            Some(value) => Some(AggregateSpec::from_json(value)?),
        };

        let exclude_rule_ids = match object.get("excludeRuleIds") {
            None => Vec::new(),
            Some(value) => {
                let array = value
                    .as_array()
                    .ok_or_else(|| SpatialError::invalid_query("'excludeRuleIds' must be an array"))?;
                array
                    .iter()
                    .map(|item| {
                        item.as_str()
                            .map(String::from)
                            .ok_or_else(|| SpatialError::invalid_query("'excludeRuleIds' must contain strings"))
                    })
                    .collect::<Result<Vec<_>, _>>()?
            }
        };

        let include_overlap = match object.get("includeOverlap") {
            None => false,
            Some(value) => value
                .as_bool()
                .ok_or_else(|| SpatialError::invalid_query("'includeOverlap' must be a boolean"))?,
        };

        Ok(Query {
            spatial,
            where_clause,
            exclude_rule_ids,
            include_overlap,
            distance_meters,
            at,
            aggregate,
        })
    }
}

/// Geodesic overlap between a matched candidate and one rule (ADR-0012).
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct OverlapMetric {
    /// Area of `candidate ∩ rule` in m² (geo's native geodesic unit).
    pub overlap_area: f64,
    /// `geodesic_area(candidate ∩ rule) / geodesic_area(candidate)`, in [0, 1].
    pub overlap_ratio: f64,
}

/// The outcome for one candidate, aligned to input order (ADR-0004).
///
/// `Matched.overlaps` is `Some(per-rule metrics, aligned to `rule_ids`)` only
/// when the query requested `includeOverlap`; otherwise `None` (ADR-0012).
/// `Matched.aggregate` is `Some` only when the query requested an
/// [`AggregateSpec`] (ADR-0018); the analytics are computed over the matched
/// rule set — the same set resolution calls applicable.
#[derive(Debug, Clone, PartialEq)]
pub enum CandidateOutcome {
    Matched {
        rule_ids: Vec<RuleId>,
        overlaps: Option<Vec<OverlapMetric>>,
        aggregate: Option<Aggregate>,
    },
    NotMatched,
    Invalid { reason: String },
}

/// One rule in the ordered applicable set — the per-rule explanation member
/// (ADR-0015). An applicable rule passed both admission gates, so
/// `spatial_matched` (the DE-9IM predicate held) and `property_matched` (the
/// `where` clause admitted the rule) are data the evaluation already computes;
/// a rule failing either gate is absent from the set.
#[derive(Debug, Clone, PartialEq)]
pub struct ApplicableRule {
    pub rule_id: RuleId,
    /// The rule's top-level precedence (higher wins).
    pub priority: i64,
    /// The query's spatial predicate held between the candidate and the rule.
    pub spatial_matched: bool,
    /// The `where` clause admitted the rule.
    pub property_matched: bool,
}

/// The resolution outcome for one candidate, aligned to input order
/// (ADR-0015). Resolution answers "which rule wins, what values apply, and
/// why" instead of only "which rules matched".
///
/// `Resolved.winner` is the head of the priority-descending applicable order;
/// `values` is the first-provider-wins merge of the applicable rules'
/// properties down that order (a field no applicable rule defines is absent);
/// `applicable` is the ordered set, which is the explanation; `aggregate` is
/// the requested analytics over that set (ADR-0018), `None` when not asked.
#[derive(Debug, Clone, PartialEq)]
pub enum ResolutionOutcome {
    Resolved {
        winner: RuleId,
        values: BTreeMap<String, PropertyValue>,
        applicable: Vec<ApplicableRule>,
        aggregate: Option<Aggregate>,
    },
    NotMatched,
    Invalid { reason: String },
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::model::where_expr::WhereExpr;

    #[test]
    fn well_formed_programmatic_query_has_no_reason() {
        let query = Query::new(SpatialPredicate::WithinDistance).with_distance(200.0);
        assert_eq!(query.validate(), None);

        let query = Query::new(SpatialPredicate::Intersects);
        assert_eq!(query.validate(), None);
    }

    #[test]
    fn distance_on_non_within_predicate_is_rejected() {
        let query = Query::new(SpatialPredicate::Intersects).with_distance(200.0);
        assert!(query
            .validate()
            .is_some_and(|r| r.contains("only valid with the 'withinDistance'")));
    }

    #[test]
    fn within_distance_requires_a_positive_finite_radius() {
        for missing in [None, Some(f64::NAN), Some(f64::INFINITY), Some(0.0), Some(-1.0)] {
            let mut query = Query::new(SpatialPredicate::WithinDistance);
            if let Some(d) = missing {
                query = query.with_distance(d);
            }
            assert!(query
                .validate()
                .is_some_and(|r| r.contains("positive distance")));
        }
    }

    #[test]
    fn active_at_requires_a_reference_time() {
        let where_clause = WhereExpr::parse(&serde_json::json!({
            "$activeAt": {
                "daysOfWeek": "d",
                "startHour": "s",
                "endHour": "e"
            }
        }))
        .unwrap();
        let query = Query::new(SpatialPredicate::Intersects).with_where(where_clause);
        assert!(query
            .validate()
            .is_some_and(|r| r.contains("'at' is required")));

        let with_at = query.with_at(TemporalInstant::parse_iso8601("2024-01-01T00:00").unwrap());
        assert_eq!(with_at.validate(), None);
    }
}
