//! Deterministic synthetic dataset for the benchmark harness (ticket 12, §31).
//!
//! ~30 country-scale MultiPolygon rules (some with holes, variable vertex
//! count, some highly complex) plus ~1,000 small polygon candidates. Generated
//! from a fixed seed so every run is reproducible.
//!
//! The geometry is representative (blobby, irregular country-like shapes), not
//! real Natural Earth data; real open data can be dropped in without changing
//! the harness.

use std::collections::BTreeMap;

use geo::{Coord, LineString, MultiPolygon, Polygon};
use spatial_rules_core::{Candidate, PropertyValue, Rule};

pub const RULE_COUNT: usize = 30;
pub const CANDIDATE_COUNT: usize = 1000;

const CLASSIFICATIONS: [&str; 5] = [
    "restricted",
    "military",
    "protected",
    "advisory",
    "prohibited",
];

/// A tiny deterministic LCG — no external RNG dependency.
struct Rng(u64);

impl Rng {
    fn new(seed: u64) -> Self {
        Rng(seed)
    }

    fn next_u64(&mut self) -> u64 {
        self.0 = self
            .0
            .wrapping_mul(6_364_136_223_846_793_005)
            .wrapping_add(1_442_695_040_888_963_407);
        self.0
    }

    fn f64(&mut self) -> f64 {
        (self.next_u64() >> 11) as f64 / ((1u64 << 53) as f64)
    }

    fn range(&mut self, low: f64, high: f64) -> f64 {
        low + (high - low) * self.f64()
    }

    fn usize(&mut self, bound: usize) -> usize {
        (self.next_u64() as usize) % bound
    }
}

/// An irregular closed ring around `(cx, cy)` with `vertices` points.
fn blob(rng: &mut Rng, cx: f64, cy: f64, radius: f64, vertices: usize) -> LineString<f64> {
    blob_with_jitter(rng, cx, cy, radius, vertices, 0.55, 1.45)
}

/// Like [`blob`], with an explicit radial jitter range. A positive radius at
/// every angle keeps the ring simple (star-shaped), so the polygon is valid.
fn blob_with_jitter(
    rng: &mut Rng,
    cx: f64,
    cy: f64,
    radius: f64,
    vertices: usize,
    jitter_lo: f64,
    jitter_hi: f64,
) -> LineString<f64> {
    let mut coords = Vec::with_capacity(vertices + 1);
    for index in 0..vertices {
        let angle = (index as f64 / vertices as f64) * std::f64::consts::TAU;
        let r = radius * (jitter_lo + (jitter_hi - jitter_lo) * rng.f64());
        coords.push(Coord {
            x: cx + r * angle.cos(),
            y: cy + r * angle.sin(),
        });
    }
    coords.push(coords[0]);
    LineString::from(coords)
}

/// ~30 country-scale rules, distributed over a coarse grid.
pub fn rules() -> Vec<Rule> {
    let mut rng = Rng::new(0x5eed_1a2b_3c4d);
    let mut rules = Vec::with_capacity(RULE_COUNT);
    for index in 0..RULE_COUNT {
        let column = (index % 6) as f64;
        let row = (index / 6) as f64;
        let cx = -150.0 + column * 60.0 + rng.range(-8.0, 8.0);
        let cy = -50.0 + row * 25.0 + rng.range(-4.0, 4.0);

        let base_radius = 5.0 + rng.f64() * 18.0;
        let parts = 1 + rng.usize(3);
        let mut polygons = Vec::with_capacity(parts);
        for part in 0..parts {
            // Spread the parts apart (island chain) so the MultiPolygon is
            // valid: parts must not overlap.
            let angle = (part as f64 / parts as f64) * std::f64::consts::TAU + rng.range(0.0, 0.5);
            let offset = base_radius * 2.5;
            let part_cx = cx + offset * angle.cos();
            let part_cy = cy + offset * angle.sin();
            let radius = base_radius * (0.5 + 0.5 * rng.f64());
            let vertices = 60 + rng.usize(340);
            let exterior = blob(&mut rng, part_cx, part_cy, radius, vertices);
            let holes = if rng.f64() < 0.35 {
                // Hole jitter keeps the hole strictly inside the exterior
                // (max hole radius 0.175*radius < min exterior 0.275*radius).
                vec![blob_with_jitter(
                    &mut rng,
                    part_cx,
                    part_cy,
                    radius * 0.25,
                    24,
                    0.3,
                    0.7,
                )]
            } else {
                vec![]
            };
            polygons.push(Polygon::new(exterior, holes));
        }

        let mut properties = BTreeMap::new();
        properties.insert("active".to_string(), PropertyValue::Bool(index % 2 == 0));
        properties.insert(
            "priority".to_string(),
            PropertyValue::Int((index % 5) as i64),
        );
        properties.insert(
            "classification".to_string(),
            PropertyValue::Str(CLASSIFICATIONS[index % 5].to_string()),
        );
        properties.insert(
            "country".to_string(),
            PropertyValue::Str(format!("C{index:02}")),
        );

        rules.push(Rule {
            id: format!("rule-{index:02}"),
            properties,
            geometry: geo::Geometry::MultiPolygon(MultiPolygon::new(polygons)),
        });
    }
    rules
}

/// ~1,000 small polygon candidates.
pub fn candidates() -> Vec<Candidate> {
    let mut rng = Rng::new(0xcafe_2026_f00d);
    let mut candidates = Vec::with_capacity(CANDIDATE_COUNT);
    for index in 0..CANDIDATE_COUNT {
        let cx = rng.range(-170.0, 170.0);
        let cy = rng.range(-55.0, 75.0);
        let width = rng.range(0.3, 2.5);
        let height = rng.range(0.3, 2.5);
        let ring = LineString::from(vec![
            Coord {
                x: cx - width / 2.0,
                y: cy - height / 2.0,
            },
            Coord {
                x: cx - width / 2.0,
                y: cy + height / 2.0,
            },
            Coord {
                x: cx + width / 2.0,
                y: cy + height / 2.0,
            },
            Coord {
                x: cx + width / 2.0,
                y: cy - height / 2.0,
            },
            Coord {
                x: cx - width / 2.0,
                y: cy - height / 2.0,
            },
        ]);
        candidates.push(Candidate::new(
            format!("candidate-{index:04}"),
            geo::Geometry::Polygon(Polygon::new(ring, vec![])),
        ));
    }
    candidates
}

/// Rules as a GeoJSON FeatureCollection (for external cross-checks).
pub fn rules_geojson() -> String {
    let features: Vec<serde_json::Value> = rules()
        .iter()
        .map(|rule| {
            let properties: serde_json::Map<String, serde_json::Value> = rule
                .properties
                .iter()
                .map(|(key, value)| (key.clone(), property_value_to_json(value)))
                .collect();
            serde_json::json!({
                "type": "Feature",
                "id": rule.id,
                "properties": properties,
                "geometry": geometry_to_geojson(&rule.geometry),
            })
        })
        .collect();
    serde_json::json!({
        "type": "FeatureCollection",
        "features": features,
    })
    .to_string()
}

/// Candidates as a GeoJSON FeatureCollection (for external cross-checks).
pub fn candidates_geojson() -> String {
    let features: Vec<serde_json::Value> = candidates()
        .iter()
        .map(|candidate| {
            serde_json::json!({
                "type": "Feature",
                "id": candidate.id,
                "properties": {},
                "geometry": geometry_to_geojson(&candidate.geometry),
            })
        })
        .collect();
    serde_json::json!({
        "type": "FeatureCollection",
        "features": features,
    })
    .to_string()
}

fn property_value_to_json(value: &PropertyValue) -> serde_json::Value {
    match value {
        PropertyValue::Null => serde_json::Value::Null,
        PropertyValue::Bool(b) => serde_json::Value::Bool(*b),
        PropertyValue::Int(i) => serde_json::Value::from(*i),
        PropertyValue::Float(f) => serde_json::Value::from(*f),
        PropertyValue::Str(s) => serde_json::Value::String(s.clone()),
    }
}

fn geometry_to_geojson(geometry: &geo::Geometry<f64>) -> serde_json::Value {
    match geometry {
        geo::Geometry::Polygon(polygon) => serde_json::json!({
            "type": "Polygon",
            "coordinates": polygon_rings(polygon),
        }),
        geo::Geometry::MultiPolygon(multi) => serde_json::json!({
            "type": "MultiPolygon",
            "coordinates": multi.0.iter().map(polygon_rings).collect::<Vec<_>>(),
        }),
        other => unreachable!("dataset generates only Polygon/MultiPolygon, got {other:?}"),
    }
}

fn polygon_rings(polygon: &Polygon<f64>) -> Vec<Vec<[f64; 2]>> {
    let mut rings = vec![ring_coords(polygon.exterior())];
    rings.extend(polygon.interiors().iter().map(ring_coords));
    rings
}

fn ring_coords(line: &LineString<f64>) -> Vec<[f64; 2]> {
    line.0.iter().map(|coord| [coord.x, coord.y]).collect()
}

#[cfg(test)]
mod tests {
    use super::*;
    use spatial_rules_core::Ruleset;

    #[test]
    fn generated_rules_build_a_valid_ruleset() {
        let generated = rules();
        assert_eq!(generated.len(), RULE_COUNT);
        Ruleset::build(generated).expect("all generated rules must be valid");
    }

    #[test]
    fn generated_candidates_are_supported() {
        assert_eq!(candidates().len(), CANDIDATE_COUNT);
    }
}
