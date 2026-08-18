//! Query model and per-candidate outcomes (ADR-0004, ADR-0005).

use std::str::FromStr;

use crate::error::SpatialError;
use crate::rule::RuleId;
use crate::where_expr::WhereExpr;

/// A spatial predicate between a candidate and a rule (ADR-0008, ADR-0012).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SpatialPredicate {
    Intersects,
    Contains,
    Within,
    Covers,
    CoveredBy,
    Touches,
    Overlaps,
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
            other => Err(SpatialError::unsupported_spatial_predicate(format!(
                "unsupported spatial predicate: {other}"
            ))),
        }
    }
}

/// One batch evaluation: a spatial predicate, an optional property `where`
/// clause, optional excluded rule ids (CONTEXT.md §5), and an opt-in overlap
/// computation (ADR-0012).
#[derive(Debug, Clone, PartialEq)]
pub struct Query {
    pub spatial: SpatialPredicate,
    pub where_clause: Option<WhereExpr>,
    pub exclude_rule_ids: Vec<String>,
    /// When true, the rich path computes per-matched-rule geodesic overlap
    /// area/ratio (ADR-0012). The hot-path mask ignores this flag.
    pub include_overlap: bool,
}

impl Query {
    pub fn new(spatial: SpatialPredicate) -> Self {
        Query {
            spatial,
            where_clause: None,
            exclude_rule_ids: Vec::new(),
            include_overlap: false,
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

    /// Parse the JSON query shape (Initial-plan §22):
    /// `{ "spatial": { "predicate": "..." }, "where": {...}, "excludeRuleIds": [...], "includeOverlap": true }`.
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

        let where_clause = match object.get("where") {
            None => None,
            Some(value) => Some(WhereExpr::parse(value)?),
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
#[derive(Debug, Clone, PartialEq)]
pub enum CandidateOutcome {
    Matched {
        rule_ids: Vec<RuleId>,
        overlaps: Option<Vec<OverlapMetric>>,
    },
    NotMatched,
    Invalid { reason: String },
}
