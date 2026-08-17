//! Candidate type — a geometry being evaluated against the rules.

use geo::Geometry;

/// A candidate geometry under evaluation (CONTEXT.md §4.2).
///
/// Only the geometry participates in matching; candidate properties are
/// imagery metadata and are not used by the engine in v1.
#[derive(Debug, Clone, PartialEq)]
pub struct Candidate {
    /// Application-supplied identifier (feature `id`).
    pub id: String,
    /// The candidate geometry (Polygon or MultiPolygon).
    pub geometry: Geometry<f64>,
}
