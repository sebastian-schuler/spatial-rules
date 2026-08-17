//! `spatial-rules-core` — the pure-Rust spatial rules/query engine core.
//!
//! The core is independent of Node/Bun and HTTP. It ingests GeoJSON features
//! into [`Rule`]s and [`Candidate`]s, gates rule geometries on validity, and
//! hosts the immutable [`Rule`]set and batch query engine in later increments.
//! See `CONTEXT.md` for the domain vocabulary and `docs/Initial-plan.md` for
//! the requirements source of truth.

mod candidate;
mod error;
mod ingestion;
mod properties;
mod rule;
mod validation;

pub use candidate::Candidate;
pub use error::{ErrorCode, SpatialError};
pub use ingestion::{
    candidate_from_feature, candidates_from_geojson, feature_geometry, parse_geojson,
    rule_from_feature, rules_from_geojson,
};
pub use properties::{properties_from_json, PropertyValue};
pub use rule::{Rule, RuleId};
pub use validation::{ensure_supported_geometry, validate_rule_geometry};
