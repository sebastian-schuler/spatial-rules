//! Shared fixtures for the core integration test suite (architecture-hardening
//! 07). Defined once so the unit-square polygon, rule/candidate builders, and
//! the jittered ring cannot drift between test files.
//!
//! Each integration test file compiles this module into its own crate, and no
//! single file uses every fixture — so dead-code warnings are expected here.

#![allow(dead_code)]

use geo::{Coord, Geometry, LineString, Polygon};
use spatial_rules_core::{Candidate, PropertyValue, Rule};

/// A closed, axis-aligned square ring from `(min_x, min_y)` to `(max_x, max_y)`.
pub fn square(min_x: f64, min_y: f64, max_x: f64, max_y: f64) -> Polygon<f64> {
    Polygon::new(
        LineString::from(vec![
            (min_x, min_y),
            (min_x, max_y),
            (max_x, max_y),
            (max_x, min_y),
            (min_x, min_y),
        ]),
        vec![],
    )
}

/// The canonical unit square (0,0)–(10,10) as a geometry.
pub fn unit_square_geometry() -> Geometry<f64> {
    Geometry::Polygon(square(0.0, 0.0, 10.0, 10.0))
}

/// A square centred on `(cx, cy)` with half-extent `half`, as a geometry.
pub fn square_around(cx: f64, cy: f64, half: f64) -> Geometry<f64> {
    Geometry::Polygon(square(cx - half, cy - half, cx + half, cy + half))
}

/// The unit square with a centred square hole.
pub fn square_with_hole() -> Polygon<f64> {
    Polygon::new(
        LineString::from(vec![
            (0.0, 0.0),
            (0.0, 10.0),
            (10.0, 10.0),
            (10.0, 0.0),
            (0.0, 0.0),
        ]),
        vec![LineString::from(vec![
            (2.0, 2.0),
            (2.0, 4.0),
            (4.0, 4.0),
            (4.0, 2.0),
            (2.0, 2.0),
        ])],
    )
}

/// A self-intersecting "bowtie" polygon (OGC-invalid).
pub fn bowtie() -> Polygon<f64> {
    Polygon::new(
        LineString::from(vec![
            (0.0, 0.0),
            (10.0, 10.0),
            (0.0, 10.0),
            (10.0, 0.0),
            (0.0, 0.0),
        ]),
        vec![],
    )
}

/// A closed, star-shaped jittered ring (radius jitter always positive → a
/// valid, non-self-intersecting ring), seeded for determinism — the same shape
/// the benchmark dataset generator uses.
pub fn jittered_ring(
    cx: f64,
    cy: f64,
    radius: f64,
    vertices: usize,
    seed: u64,
) -> LineString<f64> {
    let mut state = seed;
    let mut next = move || {
        state = state
            .wrapping_mul(6364136223846793005)
            .wrapping_add(1442695040888963407);
        ((state >> 11) as f64) / ((1u64 << 53) as f64)
    };
    let mut coords: Vec<Coord<f64>> = Vec::with_capacity(vertices + 1);
    for index in 0..vertices {
        let angle = (index as f64 / vertices as f64) * std::f64::consts::TAU;
        let r = radius * (0.7 + 0.5 * next());
        coords.push(Coord {
            x: cx + r * angle.cos(),
            y: cy + r * angle.sin(),
        });
    }
    coords.push(coords[0]);
    LineString::from(coords)
}

/// A geometry-bearing rule with no properties.
pub fn rule(id: &str, geometry: Geometry<f64>) -> Rule {
    Rule {
        id: id.to_string(),
        properties: Default::default(),
        geometry,
        priority: 0,
    }
}

/// A polygon rule with typed properties.
pub fn rule_with_props(
    id: &str,
    polygon: Polygon<f64>,
    properties: &[(&str, PropertyValue)],
) -> Rule {
    Rule {
        id: id.to_string(),
        properties: properties
            .iter()
            .map(|(key, value)| (key.to_string(), value.clone()))
            .collect(),
        geometry: Geometry::Polygon(polygon),
        priority: 0,
    }
}

/// A polygon candidate, classified at intake.
pub fn candidate(id: &str, polygon: Polygon<f64>) -> Candidate {
    Candidate::new(id.to_string(), Geometry::Polygon(polygon))
}

/// A candidate over an arbitrary geometry, classified at intake.
pub fn candidate_geometry(id: &str, geometry: Geometry<f64>) -> Candidate {
    Candidate::new(id.to_string(), geometry)
}
