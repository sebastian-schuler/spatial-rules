//! Emits predicate results for the turf.js cross-check fixtures (ticket 12,
//! ADR-0008, ADR-0012). Reads `benchmarks/data/cross_check.json` and prints,
//! for each named pair, the DE-9IM predicates the core answers (via geo
//! `Relate`), as JSON on stdout for the Node side to diff against turf.js.

use geo::Relate;

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
        let candidate = geometry_from_json(&pair["a"]);
        let rule = geometry_from_json(&pair["b"]);
        let matrix = candidate.relate(&rule);
        results.push(serde_json::json!({
            "name": name,
            "intersects": matrix.is_intersects(),
            "contains": matrix.is_contains(),
            "within": matrix.is_within(),
            "covers": matrix.is_covers(),
            "covered_by": matrix.is_coveredby(),
            "touches": matrix.is_touches(),
            "overlaps": matrix.is_overlaps(),
        }));
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
