//! `spatial-rules-core` — the pure-Rust spatial rules/query engine core.
//!
//! The core is independent of Node/Bun and HTTP. It ingests GeoJSON features
//! into [`Rule`]s and [`Candidate`]s, gates rule geometries on validity, and
//! hosts the immutable [`Rule`]set and batch query engine in later increments.
//! See `CONTEXT.md` for the domain vocabulary and `docs/Initial-plan.md` for
//! the requirements source of truth.

mod error;
#[cfg(test)]
mod test_support;

pub use error::{ErrorCode, SpatialError};

// Domain layers, grouped so a change lives in one focused module rather than a
// flat directory spanning every concern:
// - `model`: the domain types (rules, candidates, queries, properties,
//   temporal windows, predicates) plus geometry validation.
// - `indexing`: the spatial and property indexes and the prepared-geometry
//   cache.
// - `runtime`: the ruleset compilation, evaluation, aggregation, engine, and
//   the GeoJSON ingestion that builds the model.
pub(crate) mod runtime {
    pub(crate) mod access;
    pub(crate) mod aggregate;
    pub(crate) mod engine;
    pub(crate) mod evaluate;
    pub(crate) mod ingestion;
    pub(crate) mod ruleset;
}

mod indexing {
    pub(crate) mod prepared_cache;
    pub(crate) mod property_index;
    pub(crate) mod spatial_index;
}

mod model {
    pub(crate) mod aggregate;
    pub(crate) mod candidate;
    pub(crate) mod properties;
    pub(crate) mod query;
    pub(crate) mod rule;
    pub(crate) mod temporal;
    pub(crate) mod validation;
    pub(crate) mod where_expr;
}

pub use runtime::access::RuleAccess;
pub use runtime::aggregate::{Aggregate, AggregateSpec};
pub use runtime::engine::{Engine, ReplaceReport};
pub use runtime::ingestion::{
    candidate_from_feature, candidates_from_geojson, feature_geometry, parse_geojson,
    rule_from_feature, rules_from_geojson,
};
pub use runtime::ruleset::{PreparedQuery, Ruleset};
pub use model::candidate::{Candidate, CandidateClass};
pub use model::properties::{properties_from_json, PropertyValue};
pub use model::query::{ApplicableRule, CandidateOutcome, OverlapMetric, Query, ResolutionOutcome, SpatialPredicate};
pub use model::rule::{Rule, RuleId};
pub use model::temporal::TemporalInstant;
pub use model::validation::{classify_candidate, ensure_supported_geometry, validate_rule_geometry};
pub use model::where_expr::{FieldOp, FieldPredicate, WhereExpr};
// Benchmark-ladder seams are hidden from the production API: only the
// spatial-index machinery is needed by the benchmark crate to swap index
// implementations, and that crate enables the `benchmark` feature (tests enable
// it implicitly via `cfg(test)`).
#[cfg(feature = "benchmark")]
pub use runtime::ruleset::{PreparedRuleGeometries, RuleSource};
#[cfg(feature = "benchmark")]
pub use indexing::spatial_index::{build_spatial_index, LinearScanIndex, RStarIndex, SpatialIndex, SpatialIndexKind};
