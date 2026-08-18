//! Integration tests for immutable `Ruleset` compilation (ticket 14).
//!
//! Exercises the public seams: ruleset build/validation, numeric rule-id
//! mapping, precomputed envelopes, the `SpatialIndex` trait (rstar default and
//! linear-scan baseline), and the compile-time property equality/`$in` index.

use std::collections::BTreeMap;

use geo::{LineString, Point, Polygon, Rect};
use serde_json::json;
use spatial_rules_core::{
    rules_from_geojson, Candidate, CandidateOutcome, ErrorCode, PropertyValue, Query, Rule,
    RuleId, Ruleset, SpatialIndexKind, SpatialPredicate,
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
    let zone_a = ruleset.rule_id("zone-a").unwrap();
    let zone_b = ruleset.rule_id("zone-b").unwrap();
    assert_eq!(ruleset.rule_id("zone-a"), Some(zone_a));
    assert_eq!(ruleset.rule_id("zone-b"), Some(zone_b));
    assert_eq!(ruleset.rule_id("missing"), None);
    assert_eq!(ruleset.string_id(zone_a), "zone-a");
    assert_eq!(ruleset.string_id(zone_b), "zone-b");
}

#[test]
fn exposes_geometry_and_properties_by_rule_id() {
    let ruleset = Ruleset::from_geojson(GEOJSON).unwrap();
    let a = ruleset.rule_id("zone-a").unwrap();
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
    let zone_a = ruleset.rule_id("zone-a").unwrap();
    let zone_b = ruleset.rule_id("zone-b").unwrap();
    assert_eq!(*ruleset.envelope(zone_a), Rect::new((0.0, 0.0), (10.0, 10.0)));
    assert_eq!(
        *ruleset.envelope(zone_b),
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
    let zone_a = ruleset.rule_id("zone-a").unwrap();
    let zone_b = ruleset.rule_id("zone-b").unwrap();
    // Overlaps only zone-a.
    assert_eq!(
        ruleset.query_envelope(&Rect::new((5.0, 5.0), (6.0, 6.0))),
        vec![zone_a]
    );
    // Overlaps only zone-b.
    assert_eq!(
        ruleset.query_envelope(&Rect::new((105.0, 105.0), (106.0, 106.0))),
        vec![zone_b]
    );
    // Overlaps both.
    assert_eq!(
        ruleset.query_envelope(&Rect::new((-50.0, -50.0), (200.0, 200.0))),
        vec![zone_a, zone_b]
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
fn where_equality_index_filters_by_property() {
    let ruleset = Ruleset::from_geojson(GEOJSON).unwrap();
    let zone_a = ruleset.rule_id("zone-a").unwrap();
    let inside_a = Candidate {
        id: "inside-a".to_string(),
        geometry: geo::Geometry::Polygon(square(2.0, 2.0, 4.0, 4.0)),
    };

    let restricted = Query::from_json(&json!({
        "spatial": { "predicate": "intersects" },
        "where": { "classification": "restricted" }
    }))
    .unwrap();
    assert_eq!(
        ruleset.query(std::slice::from_ref(&inside_a), &restricted),
        vec![CandidateOutcome::Matched { rule_ids: vec![zone_a] }]
    );

    // A value absent from every spatially-matching rule is a non-match.
    let absent = Query::from_json(&json!({
        "spatial": { "predicate": "intersects" },
        "where": { "classification": "other" }
    }))
    .unwrap();
    assert_eq!(
        ruleset.query(&[inside_a], &absent),
        vec![CandidateOutcome::NotMatched]
    );
}

#[test]
fn where_in_index_unions_overlapping_rules() {
    let ruleset = Ruleset::build(vec![
        rule(
            "a",
            square(0.0, 0.0, 10.0, 10.0),
            &[("country", PropertyValue::Str("HR".into()))],
        ),
        rule(
            "b",
            square(5.0, 5.0, 15.0, 15.0),
            &[("country", PropertyValue::Str("SI".into()))],
        ),
    ])
    .unwrap();
    let inside = Candidate {
        id: "inside".to_string(),
        geometry: geo::Geometry::Polygon(square(6.0, 6.0, 8.0, 8.0)),
    };
    let query = Query::from_json(&json!({
        "spatial": { "predicate": "intersects" },
        "where": { "country": { "$in": ["HR", "SI"] } }
    }))
    .unwrap();
    let outcomes = ruleset.query(&[inside], &query);
    let CandidateOutcome::Matched { rule_ids: matched } = &outcomes[0] else {
        panic!("expected a match");
    };
    assert_eq!(
        matched.as_slice(),
        &[ruleset.rule_id("a").unwrap(), ruleset.rule_id("b").unwrap()]
    );
}

#[test]
fn empty_ruleset_is_valid() {
    let ruleset = Ruleset::build(vec![]).unwrap();
    assert!(ruleset.is_empty());
    assert_eq!(
        ruleset.query_envelope(&Rect::new((0.0, 0.0), (10.0, 10.0))),
        Vec::<RuleId>::new()
    );
    let candidate = Candidate {
        id: "c".to_string(),
        geometry: geo::Geometry::Polygon(square(0.0, 0.0, 1.0, 1.0)),
    };
    assert_eq!(
        ruleset.query(&[candidate], &Query::new(SpatialPredicate::Intersects)),
        vec![CandidateOutcome::NotMatched]
    );
}

#[test]
fn from_geojson_rejects_malformed_input() {
    let err = Ruleset::from_geojson("not json").unwrap_err();
    assert_eq!(err.code, ErrorCode::InvalidGeoJson);
}

#[test]
fn rule_source_iterates_in_ruleset_order() {
    let ruleset = Ruleset::from_geojson(GEOJSON).unwrap();
    let zone_a = ruleset.rule_id("zone-a").unwrap();
    let zone_b = ruleset.rule_id("zone-b").unwrap();

    let collected: Vec<_> = ruleset.rules().iter().collect();
    assert_eq!(collected.len(), 2);
    assert_eq!(collected[0].0, zone_a);
    assert_eq!(*collected[0].2, Rect::new((0.0, 0.0), (10.0, 10.0)));
    assert_eq!(collected[1].0, zone_b);
    assert_eq!(*collected[1].2, Rect::new((100.0, 100.0), (110.0, 110.0)));
}
