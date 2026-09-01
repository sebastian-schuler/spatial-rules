//! Query model and per-candidate outcomes (ADR-0004, ADR-0005, ADR-0015).

use std::collections::BTreeMap;
use std::str::FromStr;

use crate::aggregate::{Aggregate, AggregateSpec};
use crate::error::SpatialError;
use crate::properties::PropertyValue;
use crate::rule::RuleId;
use crate::temporal::TemporalInstant;
use crate::where_expr::WhereExpr;

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
                let instant = TemporalInstant::parse_iso8601(text)
                    .map_err(|e| SpatialError::invalid_query(format!("invalid 'at': {e}")))?;
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
