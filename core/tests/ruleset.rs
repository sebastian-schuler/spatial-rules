//! Integration tests for immutable `Ruleset` compilation (ticket 14).
//!
//! Exercises the public seams: ruleset build/validation, numeric rule-id
//! mapping, precomputed envelopes, the `SpatialIndex` trait (rstar default and
//! linear-scan baseline), and the compile-time property equality/`$in` index.

use std::collections::BTreeMap;

use geo::{LineString, Point, Polygon, Rect};
use spatial_rules_core::{
    rules_from_geojson, ErrorCode, PropertyValue, Rule, RuleId, Ruleset, SpatialIndexKind,
};

const GEOJSON: &str = r#"{
  "type": "FeatureCollection",
  "features": [
    {
      "type": "Feature",
      "id": "zone-a",
      "properties": { "active": true, "classification": "restricted", "country": "HR" },
      "geometry": { "type": "Polygon", "coordinates": [[[0, 0], [0, 10], [10, 10], [10, 0], [0, 0]]] }
    },
    {
      "type": "Feature",
      "id": "zone-b",
      "properties": { "active": false, "classification": "military", "country": "SI" },
      "geometry": { "type": "Polygon", "coordinates": [[[100, 100], [100, 110], [110, 110], [110, 100], [100, 100]]] }
    }
  ]
}"#;

fn square(min_x: f64, min_y: f64, max_x: f64, max_y: f64) -> Polygon<f64> {
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

fn rule(id: &str, polygon: Polygon<f64>, properties: &[(&str, PropertyValue)]) -> Rule {
    Rule {
        id: id.to_string(),
        properties: properties
            .iter()
            .map(|(key, value)| (key.to_string(), value.clone()))
            .collect(),
        geometry: geo::Geometry::Polygon(polygon),
    }
}

#[test]
fn builds_ruleset_from_geojson() {
    let ruleset = Ruleset::from_geojson(GEOJSON).unwrap();
    assert_eq!(ruleset.len(), 2);
    assert!(!ruleset.is_empty());
    assert_eq!(ruleset.rule_id("zone-a"), Some(RuleId(0)));
    assert_eq!(ruleset.rule_id("zone-b"), Some(RuleId(1)));
    assert_eq!(ruleset.rule_id("missing"), None);
    assert_eq!(ruleset.string_id(RuleId(0)), "zone-a");
    assert_eq!(ruleset.string_id(RuleId(1)), "zone-b");
}

#[test]
fn exposes_geometry_and_properties_by_rule_id() {
    let ruleset = Ruleset::from_geojson(GEOJSON).unwrap();
    let a = RuleId(0);
    assert_eq!(
        ruleset.geometry(a),
        &geo::Geometry::Polygon(square(0.0, 0.0, 10.0, 10.0))
    );
    assert_eq!(
        ruleset.properties(a).get("classification"),
        Some(&PropertyValue::Str("restricted".into()))
    );
    assert_eq!(
        ruleset.properties(a).get("active"),
        Some(&PropertyValue::Bool(true))
    );
}

#[test]
fn exposes_precomputed_envelopes() {
    let ruleset = Ruleset::from_geojson(GEOJSON).unwrap();
    assert_eq!(*ruleset.envelope(RuleId(0)), Rect::new((0.0, 0.0), (10.0, 10.0)));
    assert_eq!(
        *ruleset.envelope(RuleId(1)),
        Rect::new((100.0, 100.0), (110.0, 110.0))
    );
}

#[test]
fn rejects_invalid_rule_geometry() {
    let bowtie = Polygon::new(
        LineString::from(vec![
            (0.0, 0.0),
            (10.0, 10.0),
            (0.0, 10.0),
            (10.0, 0.0),
            (0.0, 0.0),
        ]),
        vec![],
    );
    let err = Ruleset::build(vec![rule("bad", bowtie, &[])]).unwrap_err();
    assert_eq!(err.code, ErrorCode::InvalidGeometry);
    assert!(err.message.contains("bad"));
}

#[test]
fn rejects_unsupported_geometry_type() {
    let rules = vec![Rule {
        id: "point".into(),
        properties: BTreeMap::new(),
        geometry: geo::Geometry::Point(Point::new(0.0, 0.0)),
    }];
    let err = Ruleset::build(rules).unwrap_err();
    assert_eq!(err.code, ErrorCode::UnsupportedGeometryType);
}

#[test]
fn rejects_duplicate_rule_ids() {
    let rules = vec![
        rule("dup", square(0.0, 0.0, 1.0, 1.0), &[]),
        rule("dup", square(2.0, 2.0, 3.0, 3.0), &[]),
    ];
    let err = Ruleset::build(rules).unwrap_err();
    assert_eq!(err.code, ErrorCode::RulesetConstructionFailed);
}

#[test]
fn spatial_index_returns_intersecting_rule_ids() {
    let ruleset = Ruleset::from_geojson(GEOJSON).unwrap();
    // Overlaps only zone-a.
    assert_eq!(
        ruleset.query_envelope(&Rect::new((5.0, 5.0), (6.0, 6.0))),
        vec![RuleId(0)]
    );
    // Overlaps only zone-b.
    assert_eq!(
        ruleset.query_envelope(&Rect::new((105.0, 105.0), (106.0, 106.0))),
        vec![RuleId(1)]
    );
    // Overlaps both.
    assert_eq!(
        ruleset.query_envelope(&Rect::new((-50.0, -50.0), (200.0, 200.0))),
        vec![RuleId(0), RuleId(1)]
    );
    // Overlaps neither.
    assert_eq!(
        ruleset.query_envelope(&Rect::new((50.0, 50.0), (60.0, 60.0))),
        Vec::<RuleId>::new()
    );
}

#[test]
fn linear_scan_matches_rstar() {
    let rules = rules_from_geojson(GEOJSON).unwrap();
    let rstar = Ruleset::build_with(rules.clone(), SpatialIndexKind::RStar).unwrap();
    let scan = Ruleset::build_with(rules, SpatialIndexKind::LinearScan).unwrap();

    let envelopes = [
        Rect::new((5.0, 5.0), (6.0, 6.0)),
        Rect::new((105.0, 105.0), (106.0, 106.0)),
        Rect::new((-50.0, -50.0), (200.0, 200.0)),
        Rect::new((50.0, 50.0), (60.0, 60.0)),
    ];
    for envelope in envelopes {
        assert_eq!(
            rstar.query_envelope(&envelope),
            scan.query_envelope(&envelope),
            "index mismatch for envelope {:?}",
            envelope
        );
    }
}

#[test]
fn property_index_equality_lookup() {
    let ruleset = Ruleset::from_geojson(GEOJSON).unwrap();
    let index = ruleset.property_index();
    assert_eq!(
        index.matching("classification", &PropertyValue::Str("restricted".into())),
        &[RuleId(0)]
    );
    assert_eq!(
        index.matching("classification", &PropertyValue::Str("military".into())),
        &[RuleId(1)]
    );
    assert_eq!(
        index.matching("active", &PropertyValue::Bool(true)),
        &[RuleId(0)]
    );
    assert_eq!(
        index.matching("active", &PropertyValue::Bool(false)),
        &[RuleId(1)]
    );
}

#[test]
fn property_index_in_union() {
    let ruleset = Ruleset::from_geojson(GEOJSON).unwrap();
    let index = ruleset.property_index();
    let matched = index.matching_in(
        "country",
        &[
            PropertyValue::Str("HR".into()),
            PropertyValue::Str("SI".into()),
        ],
    );
    assert_eq!(matched, vec![RuleId(0), RuleId(1)]);
}

#[test]
fn property_index_absent_value_returns_empty() {
    let ruleset = Ruleset::from_geojson(GEOJSON).unwrap();
    let index = ruleset.property_index();
    assert!(index
        .matching("classification", &PropertyValue::Str("other".into()))
        .is_empty());
    assert!(index
        .matching("missing", &PropertyValue::Bool(true))
        .is_empty());
}

#[test]
fn property_index_lists_indexed_names() {
    let ruleset = Ruleset::from_geojson(GEOJSON).unwrap();
    let mut names: Vec<&str> = ruleset.property_index().property_names().collect();
    names.sort_unstable();
    assert_eq!(names, vec!["active", "classification", "country"]);
}

#[test]
fn empty_ruleset_is_valid() {
    let ruleset = Ruleset::build(vec![]).unwrap();
    assert!(ruleset.is_empty());
    assert_eq!(
        ruleset.query_envelope(&Rect::new((0.0, 0.0), (10.0, 10.0))),
        Vec::<RuleId>::new()
    );
    assert_eq!(ruleset.property_index().len(), 0);
}

#[test]
fn from_geojson_rejects_malformed_input() {
    let err = Ruleset::from_geojson("not json").unwrap_err();
    assert_eq!(err.code, ErrorCode::InvalidGeoJson);
}
