//! `spatial-rules-core` — the pure-Rust spatial rules/query engine core.
//!
//! The core is independent of Node/Bun and HTTP. It ingests GeoJSON features
//! into [`Rule`]s and [`Candidate`]s, gates rule geometries on validity, and
//! hosts the immutable [`Rule`]set and batch query engine in later increments.
//! See `CONTEXT.md` for the domain vocabulary and `docs/Initial-plan.md` for
//! the requirements source of truth.

mod aggregate;
mod candidate;
mod engine;
mod error;
mod evaluate;
mod ingestion;
mod prepared_cache;
mod properties;
mod property_index;
mod query;
mod rule;
mod ruleset;
mod spatial_index;
#[cfg(test)]
mod test_support;
mod temporal;
mod validation;
mod where_expr;

pub use aggregate::{Aggregate, AggregateSpec};
pub use candidate::{Candidate, CandidateClass};
pub use engine::{Engine, ReplaceReport};
pub use error::{ErrorCode, SpatialError};
pub use ingestion::{
    candidate_from_feature, candidates_from_geojson, feature_geometry, parse_geojson,
    rule_from_feature, rules_from_geojson,
};
pub use properties::{properties_from_json, PropertyValue};
pub use query::{ApplicableRule, CandidateOutcome, OverlapMetric, Query, ResolutionOutcome, SpatialPredicate};
pub use rule::{Rule, RuleId};
pub use ruleset::{PreparedQuery, PreparedRuleGeometries, RuleSource, Ruleset};
pub use spatial_index::{build_spatial_index, LinearScanIndex, RStarIndex, SpatialIndex, SpatialIndexKind};
pub use temporal::TemporalInstant;
pub use validation::{classify_candidate, ensure_supported_geometry, validate_rule_geometry};
pub use where_expr::{FieldOp, FieldPredicate, WhereExpr};
