//! Emits the engine's spatial-predicate verdicts for the turf.js cross-check
//! fixtures (ticket 12, ADR-0008, ADR-0012). Reads
//! `benchmarks/data/cross_check.json` and, for each named pair, drives the
//! engine through its public seams — a single-rule ruleset and a single
//! candidate, queried once per predicate — so the cross-check certifies the
//! engine's `SpatialPredicate → DE-9IM` mapping (architecture-hardening 08),
//! not just geo's raw matrix helpers.

use spatial_rules_core::{Candidate, Query, Rule, Ruleset, SpatialPredicate};

const PREDICATES: [SpatialPredicate; 7] = [
    SpatialPredicate::Intersects,
    SpatialPredicate::Contains,
    SpatialPredicate::Within,
    SpatialPredicate::Covers,
    SpatialPredicate::CoveredBy,
    SpatialPredicate::Touches,
    SpatialPredicate::Overlaps,
];

fn main() {
    let path = std::env::args()
        .nth(1)
        .unwrap_or_else(|| "benchmarks/data/cross_check.json".to_string());
    let text = std::fs::read_to_string(&path).expect("read cross_check.json");
    let root: serde_json::Value = serde_json::from_str(&text).expect("parse cross_check.json");
    let pairs = root["pairs"].as_array().expect("pairs array");

    let mut results = Vec::new();
    for pair in pairs {
        let name = pair["name"].as_str().expect("pair name");
        let candidate = Candidate::new("a".to_string(), geometry_from_json(&pair["a"]));
        let rule = Rule {
            id: "b".to_string(),
            properties: Default::default(),
            geometry: geometry_from_json(&pair["b"]),
        };
        let ruleset = Ruleset::build(vec![rule]).expect("fixture rule must build");

        let mut result = serde_json::Map::new();
        result.insert(
            "name".to_string(),
            serde_json::Value::String(name.to_string()),
        );
        for predicate in PREDICATES {
            let mask = ruleset.query_mask(
                std::slice::from_ref(&candidate),
                &Query::new(predicate),
            );
            // The engine mask: 1 = matched, 0 = not matched, 2 = invalid.
            result.insert(
                predicate.as_str().to_string(),
                serde_json::Value::Bool(mask[0] == 1),
            );
        }
        results.push(serde_json::Value::Object(result));
    }

    println!("{}", serde_json::json!({ "pairs": results }));
}

fn geometry_from_json(value: &serde_json::Value) -> geo::Geometry<f64> {
    let geojson: geojson::GeoJson = serde_json::from_value(value.clone()).expect("GeoJSON");
    let geojson::GeoJson::Geometry(geometry) = geojson else {
        panic!("expected a geometry object");
    };
    geo::Geometry::try_from(&geometry).expect("convert to geo geometry")
}
