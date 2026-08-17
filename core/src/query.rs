//! Query model and per-candidate outcomes (ADR-0004, ADR-0005).

use std::str::FromStr;

use crate::error::SpatialError;
use crate::rule::RuleId;
use crate::where_expr::WhereExpr;

/// A spatial predicate between a candidate and a rule (ADR-0008).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SpatialPredicate {
    Intersects,
    Contains,
    Within,
}

impl SpatialPredicate {
    pub fn as_str(self) -> &'static str {
        match self {
            SpatialPredicate::Intersects => "intersects",
            SpatialPredicate::Contains => "contains",
            SpatialPredicate::Within => "within",
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
            other => Err(SpatialError::unsupported_spatial_predicate(format!(
                "unsupported spatial predicate: {other}"
            ))),
        }
    }
}

/// One batch evaluation: a spatial predicate, an optional property `where`
/// clause, and optional excluded rule ids (CONTEXT.md §5).
#[derive(Debug, Clone, PartialEq)]
pub struct Query {
    pub spatial: SpatialPredicate,
    pub where_clause: Option<WhereExpr>,
    pub exclude_rule_ids: Vec<String>,
}

impl Query {
    pub fn new(spatial: SpatialPredicate) -> Self {
        Query {
            spatial,
            where_clause: None,
            exclude_rule_ids: Vec::new(),
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

    /// Parse the JSON query shape (Initial-plan §22):
    /// `{ "spatial": { "predicate": "..." }, "where": {...}, "excludeRuleIds": [...] }`.
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

        Ok(Query {
            spatial,
            where_clause,
            exclude_rule_ids,
        })
    }
}

/// The outcome for one candidate, aligned to input order (ADR-0004).
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum CandidateOutcome {
    Matched { rule_ids: Vec<RuleId> },
    NotMatched,
    Invalid { reason: String },
}
