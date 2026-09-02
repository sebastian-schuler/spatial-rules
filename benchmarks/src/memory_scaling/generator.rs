//! Deterministic geometry generators for the memory-scaling benchmark.
//!
//! Produces the rules and candidates a scale cell measures. Both are
//! deterministic for a given [`Scale`]: same seed, same layout, same shapes, so
//! a cell's measurements are reproducible and comparable across runs.

use std::collections::BTreeMap;

use geo::{Coord, LineString, MultiPolygon, Polygon};

use crate::memory_scaling::report::Scale;
use spatial_rules_core::{Candidate, PropertyValue, Rule};

/// Deterministic star-shaped ring around `(cx, cy)` with exactly `vertices`
/// distinct points plus the closing repeat. Positive radius at every angle
/// keeps the ring simple, so the polygon passes strict validation (ADR-0005).
fn ring(rng: &mut crate::dataset::Rng, cx: f64, cy: f64, vertices: usize) -> LineString<f64> {
    let mut coords = Vec::with_capacity(vertices + 1);
    for index in 0..vertices {
        let angle = (index as f64 / vertices as f64) * std::f64::consts::TAU;
        // Jitter keeps the shape irregular but always positive-radius
        // (star-shaped ⇒ simple ⇒ valid), mirroring dataset.rs's blobs.
        let radius = 1.0 + 0.45 * rng.f64();
        coords.push(Coord {
            x: cx + radius * angle.cos(),
            y: cy + radius * angle.sin(),
        });
    }
    coords.push(coords[0]);
    LineString::from(coords)
}

/// Generate `scale.rules` valid MultiPolygon rules laid out on a coarse grid
/// so envelopes don't fully overlap (the rstar index must be able to prune).
/// Deterministic for a given [`Scale`]: same seed, same layout, same shapes.
pub fn generate_rules(scale: Scale) -> Vec<Rule> {
    let mut rng = crate::dataset::Rng::new(0x5EED_1A2B_3C4D);
    let columns = (scale.rules as f64).sqrt().ceil() as usize;
    let pitch = 10.0_f64;
    (0..scale.rules)
        .map(|index| {
            let column = (index % columns.max(1)) as f64;
            let row = (index / columns.max(1)) as f64;
            let cx = column * pitch;
            let cy = row * pitch;

            let mut properties = BTreeMap::new();
            properties.insert("active".to_string(), PropertyValue::Bool(index % 2 == 0));
            properties.insert(
                "classification".to_string(),
                PropertyValue::Str(format!("c{}", index % 5)),
            );

            Rule {
                id: format!("rule-{index:06}"),
                properties,
                geometry: geo::Geometry::MultiPolygon(MultiPolygon::new(vec![Polygon::new(
                    ring(&mut rng, cx, cy, scale.vertices),
                    vec![],
                )])),
                priority: 0,
            }
        })
        .collect()
}

/// Generate `count` small square candidates scattered over the same grid
/// extent the rules occupy, deterministically.
pub fn generate_candidates(count: usize, scale: Scale) -> Vec<Candidate> {
    let mut rng = crate::dataset::Rng::new(0xCAFE_2026_F00D);
    let columns = (scale.rules as f64).sqrt().ceil() as usize;
    let extent = columns.max(1) as f64 * 10.0;
    (0..count)
        .map(|index| {
            let cx = rng.f64() * extent - extent / 2.0;
            let cy = rng.f64() * extent - extent / 2.0;
            let half = 0.25;
            let corners = [
                (cx - half, cy - half),
                (cx - half, cy + half),
                (cx + half, cy + half),
                (cx + half, cy - half),
                (cx - half, cy - half),
            ];
            let line = LineString::from(
                corners
                    .iter()
                    .map(|(x, y)| Coord { x: *x, y: *y })
                    .collect::<Vec<_>>(),
            );
            Candidate::new(
                format!("candidate-{index:06}"),
                geo::Geometry::Polygon(Polygon::new(line, vec![])),
            )
        })
        .collect()
}
