//! Integration tests for immutable `Ruleset` compilation (ticket 14).
//!
//! Exercises the public seams: ruleset build/validation, numeric rule-id
//! mapping, precomputed envelopes, the `SpatialIndex` trait (rstar default and
//! linear-scan baseline), and the compile-time property equality/`$in` index.

use std::collections::BTreeMap;

use geo::{Point, Rect};
use serde_json::json;
use spatial_rules_core::{
    rules_from_geojson, CandidateOutcome, ErrorCode, PropertyValue, Query, Rule, RuleId, Ruleset,
    SpatialIndexKind, SpatialPredicate,
};

mod common;
use common::{bowtie, candidate, rule_with_props as rule, square};

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
    let err = Ruleset::build(vec![rule("bad", bowtie(), &[])]).unwrap_err();
    assert_eq!(err.code, ErrorCode::InvalidGeometry);
    assert!(err.message.contains("bad"));
}

#[test]
fn rejects_unsupported_geometry_type() {
    let rules = vec![Rule {
        id: "point".into(),
        properties: BTreeMap::new(),
        geometry: geo::Geometry::Point(Point::new(0.0, 0.0)),
        priority: 0,
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
    let inside_a = candidate("inside-a", square(2.0, 2.0, 4.0, 4.0));

    let restricted = Query::from_json(&json!({
        "spatial": { "predicate": "intersects" },
        "where": { "classification": "restricted" }
    }))
    .unwrap();
    assert_eq!(
        ruleset.query(std::slice::from_ref(&inside_a), &restricted),
        vec![CandidateOutcome::Matched { rule_ids: vec![zone_a], overlaps: None }]
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
    let inside = candidate("inside", square(6.0, 6.0, 8.0, 8.0));
    let query = Query::from_json(&json!({
        "spatial": { "predicate": "intersects" },
        "where": { "country": { "$in": ["HR", "SI"] } }
    }))
    .unwrap();
    let outcomes = ruleset.query(&[inside], &query);
    let CandidateOutcome::Matched { rule_ids: matched, .. } = &outcomes[0] else {
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
    let candidate = candidate("c", square(0.0, 0.0, 1.0, 1.0));
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

#[test]
fn canonical_round_trip_preserves_rules() {
    let original = Ruleset::from_geojson(GEOJSON).unwrap();
    let bytes = original.to_canonical().unwrap();
    let loaded = Ruleset::from_canonical(&bytes).unwrap();

    assert_eq!(loaded.len(), original.len());
    for (source_id, source_geometry, source_envelope) in original.rules().iter() {
        let id = original.string_id(source_id);
        let target_id = loaded.rule_id(id).expect("rule id survives the round trip");
        assert_eq!(loaded.geometry(target_id), source_geometry);
        assert_eq!(*loaded.envelope(target_id), *source_envelope);
        assert_eq!(
            loaded.properties(target_id),
            original.properties(source_id)
        );
    }
}

#[test]
fn from_canonical_rejects_malformed_input() {
    let err = Ruleset::from_canonical(b"not json").unwrap_err();
    assert_eq!(err.code, ErrorCode::InvalidGeoJson);
}

#[test]
fn from_canonical_rejects_invalid_geometry() {
    // A canonical ruleset holding a self-intersecting bowtie must fail the
    // same validation as any other load path (ADR-0013).
    let bad_rule = Rule {
        id: "bad".to_string(),
        properties: BTreeMap::new(),
        geometry: geo::Geometry::Polygon(bowtie()),
        priority: 0,
    };
    let bytes = serde_json::to_vec(&vec![bad_rule]).unwrap();
    let err = Ruleset::from_canonical(&bytes).unwrap_err();
    assert_eq!(err.code, ErrorCode::InvalidGeometry);
}

// --- Ticket 01: top-level priority hoist + canonical round-trip (ADR-0015) ---

fn rule_with_priority(id: &str, priority: i64) -> Rule {
    Rule {
        id: id.to_string(),
        properties: BTreeMap::new(),
        geometry: geo::Geometry::Polygon(square(0.0, 0.0, 10.0, 10.0)),
        priority,
    }
}

#[test]
fn ruleset_hoists_priority_aligned_to_rule_id() {
    let ruleset = Ruleset::build(vec![
        rule_with_priority("low", 5),
        rule_with_priority("high", 10),
        rule_with_priority("unprioritized", 0),
    ])
    .unwrap();
    assert_eq!(ruleset.priority(ruleset.rule_id("low").unwrap()), 5);
    assert_eq!(ruleset.priority(ruleset.rule_id("high").unwrap()), 10);
    assert_eq!(ruleset.priority(ruleset.rule_id("unprioritized").unwrap()), 0);
}

#[test]
fn canonical_round_trip_preserves_priority() {
    let original = Ruleset::build(vec![
        rule_with_priority("a", 7),
        rule_with_priority("b", 0),
    ])
    .unwrap();
    let bytes = original.to_canonical().unwrap();
    let loaded = Ruleset::from_canonical(&bytes).unwrap();
    for id in ["a", "b"] {
        let rule_id = loaded.rule_id(id).unwrap();
        assert_eq!(
            loaded.priority(rule_id),
            original.priority(original.rule_id(id).unwrap()),
            "priority of {id}"
        );
    }
}

#[test]
fn canonical_form_serializes_priority_field() {
    let original = Ruleset::build(vec![rule_with_priority("a", 7)]).unwrap();
    let bytes = original.to_canonical().unwrap();
    let text = String::from_utf8(bytes).unwrap();
    assert!(text.contains("\"priority\":7"), "canonical text: {text}");
}

#[test]
fn old_canonical_without_priority_loads_as_zero() {
    // A pre-P1 canonical rule record (no `priority` member) must load as 0
    // (ADR-0013 compatibility, ADR-0015 `#[serde(default)]`). Build it by
    // serializing a rule and stripping the `priority` member, so the fixture
    // matches geo's exact canonical geometry form without hand-writing it.
    let rule = rule_with_priority("legacy", 7);
    let mut value = serde_json::to_value(rule).unwrap();
    value
        .as_object_mut()
        .unwrap()
        .remove("priority")
        .expect("serialized rule carries priority");
    let old_style = serde_json::to_vec(&vec![value]).unwrap();

    let loaded = Ruleset::from_canonical(&old_style).unwrap();
    assert_eq!(loaded.priority(loaded.rule_id("legacy").unwrap()), 0);
}

#[test]
fn from_canonical_rejects_wrong_typed_priority_naming_the_rule() {
    // A wrong-typed `priority` in canonical input fails build with the same
    // code and rule name as the GeoJSON ingestion gate (ADR-0015), not a
    // generic parse error.
    let rule = rule_with_priority("hi", 7);
    let mut value = serde_json::to_value(rule).unwrap();
    value
        .as_object_mut()
        .unwrap()
        .insert("priority".to_string(), json!("high"));
    let bytes = serde_json::to_vec(&vec![value]).unwrap();

    let err = Ruleset::from_canonical(&bytes).unwrap_err();
    assert_eq!(err.code, ErrorCode::RulesetConstructionFailed);
    assert!(err.message.contains("hi"));
}

#[test]
fn from_canonical_rejects_negative_priority_naming_the_rule() {
    // Same gate as GeoJSON ingestion: a negative priority would silently sort
    // below unprioritized (0) rules, so it fails build (ADR-0015).
    let rule = rule_with_priority("hi", 7);
    let mut value = serde_json::to_value(rule).unwrap();
    value
        .as_object_mut()
        .unwrap()
        .insert("priority".to_string(), json!(-5));
    let bytes = serde_json::to_vec(&vec![value]).unwrap();

    let err = Ruleset::from_canonical(&bytes).unwrap_err();
    assert_eq!(err.code, ErrorCode::RulesetConstructionFailed);
    assert!(err.message.contains("hi"));
}
