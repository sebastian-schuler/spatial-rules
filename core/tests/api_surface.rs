//! Direct coverage for the exported functions of `spatial-rules-core` that the
//! feature-area tests exercise only indirectly (ticket 07 follow-up). Each test
//! targets one public function at its seam, so the "every function is tested"
//! gate is enforceable and a refactor that drops a function cannot silently
//! lose coverage.

use std::str::FromStr;

use geo::{Geometry, LineString, Point, Rect};
use serde_json::json;
use spatial_rules_core::{
    build_spatial_index, candidate_from_feature, classify_candidate, parse_geojson,
    rule_from_feature, rules_from_geojson, CandidateClass, Engine, ErrorCode, LinearScanIndex,
    Query, RuleId, RStarIndex, Ruleset, SpatialError, SpatialIndex, SpatialIndexKind,
    SpatialPredicate, WhereExpr,
};

mod common;
use common::{bowtie, candidate, rule, square, unit_square_geometry};

const TWO_RULES: &str = r#"{
  "type": "FeatureCollection",
  "features": [
    {
      "type": "Feature",
      "id": "zone-a",
      "properties": { "active": true, "country": "HR" },
      "geometry": { "type": "Polygon", "coordinates": [[[0, 0], [0, 10], [10, 10], [10, 0], [0, 0]]] }
    },
    {
      "type": "Feature",
      "id": "zone-b",
      "properties": { "active": false },
      "geometry": { "type": "Polygon", "coordinates": [[[100, 100], [100, 110], [110, 110], [110, 100], [100, 100]]] }
    }
  ]
}"#;

/// Parse a FeatureCollection and hand back its features for the direct
/// `*_from_feature` tests below.
fn features_of(input: &str) -> Vec<geojson::Feature> {
    match parse_geojson(input).unwrap() {
        geojson::GeoJson::FeatureCollection(collection) => collection.features,
        _ => panic!("expected a FeatureCollection"),
    }
}

#[test]
fn rule_from_feature_extracts_id_geometry_and_properties() {
    let features = features_of(TWO_RULES);
    let rule = rule_from_feature(&features[0]).unwrap();
    assert_eq!(rule.id, "zone-a");
    assert_eq!(rule.geometry, Geometry::Polygon(square(0.0, 0.0, 10.0, 10.0)));
    assert_eq!(
        rule.properties.get("active"),
        Some(&spatial_rules_core::PropertyValue::Bool(true))
    );
}

#[test]
fn candidate_from_feature_classifies_at_intake() {
    let features = features_of(TWO_RULES);
    let candidate = candidate_from_feature(&features[0]).unwrap();
    assert_eq!(candidate.id, "zone-a");
    match candidate.class() {
        CandidateClass::Valid { envelope } => {
            assert_eq!(*envelope, Rect::new((0.0, 0.0), (10.0, 10.0)));
        }
        CandidateClass::Invalid { .. } => panic!("a valid square must classify valid"),
    }
}

#[test]
fn numeric_feature_id_is_stringified() {
    let rules = rules_from_geojson(
        r#"{ "type": "FeatureCollection", "features": [
            { "type": "Feature", "id": 42, "properties": {},
              "geometry": { "type": "Polygon", "coordinates": [[[0,0],[0,1],[1,1],[1,0],[0,0]]] } }
        ] }"#,
    )
    .unwrap();
    assert_eq!(rules[0].id, "42");
}

#[test]
fn build_spatial_index_both_kinds_agree() {
    let ruleset = Ruleset::from_geojson(TWO_RULES).unwrap();
    let entries: Vec<(Rect<f64>, RuleId)> = ruleset
        .rules()
        .iter()
        .map(|(id, _, envelope)| (*envelope, id))
        .collect();

    let rstar = build_spatial_index(SpatialIndexKind::RStar, entries.clone());
    let scan = build_spatial_index(SpatialIndexKind::LinearScan, entries);

    let probes = [
        Rect::new((5.0, 5.0), (6.0, 6.0)),
        Rect::new((105.0, 105.0), (106.0, 106.0)),
        Rect::new((-50.0, -50.0), (200.0, 200.0)),
        Rect::new((50.0, 50.0), (60.0, 60.0)),
    ];
    for probe in probes {
        assert_eq!(rstar.query_envelope(&probe), scan.query_envelope(&probe));
    }
}

#[test]
fn concrete_index_builders_are_directly_callable() {
    let ruleset = Ruleset::from_geojson(TWO_RULES).unwrap();
    let entries: Vec<(Rect<f64>, RuleId)> = ruleset
        .rules()
        .iter()
        .map(|(id, _, envelope)| (*envelope, id))
        .collect();

    // The public constructors build the same result as the dispatcher.
    let rstar = RStarIndex::build(entries.clone());
    let scan = LinearScanIndex::build(entries);
    let probe = Rect::new((5.0, 5.0), (6.0, 6.0));
    assert_eq!(rstar.query_envelope(&probe), scan.query_envelope(&probe));
}

#[test]
fn ruleset_query_envelope_into_dedups_and_reuses_the_buffer() {
    let ruleset = Ruleset::from_geojson(TWO_RULES).unwrap();
    let zone_a = ruleset.rule_id("zone-a").unwrap();
    let zone_b = ruleset.rule_id("zone-b").unwrap();

    // The caller-owned buffer is cleared before the result is filled.
    let mut out = vec![zone_b];
    ruleset.query_envelope_into(&Rect::new((5.0, 5.0), (6.0, 6.0)), &mut out);
    assert_eq!(out, vec![zone_a]);
}

#[test]
fn query_envelope_into_dedups_and_reuses_the_buffer() {
    let ruleset = Ruleset::from_geojson(TWO_RULES).unwrap();
    let zone_a = ruleset.rule_id("zone-a").unwrap();
    let zone_b = ruleset.rule_id("zone-b").unwrap();
    let envelope_a = *ruleset.envelope(zone_a).expect("minted by this ruleset");

    // Duplicate entries mapping to one rule id must be deduplicated.
    let index = build_spatial_index(
        SpatialIndexKind::RStar,
        vec![(envelope_a, zone_a), (envelope_a, zone_a)],
    );

    // The caller-owned buffer is cleared before the result is filled.
    let mut out = vec![zone_b];
    index.query_envelope_into(&envelope_a, &mut out);
    assert_eq!(out, vec![zone_a]);
}

#[test]
fn query_builder_methods_compose() {
    let where_clause = WhereExpr::parse(&json!({ "active": true })).unwrap();
    let query = Query::new(SpatialPredicate::Intersects)
        .with_where(where_clause)
        .with_exclusions(vec!["zone-b".to_string()])
        .with_overlap();

    assert_eq!(query.spatial, SpatialPredicate::Intersects);
    assert!(query.where_clause.is_some());
    assert_eq!(query.exclude_rule_ids, vec!["zone-b".to_string()]);
    assert!(query.include_overlap);
}

#[test]
fn every_predicate_has_a_stable_string_round_trip() {
    for predicate in [
        SpatialPredicate::Intersects,
        SpatialPredicate::Contains,
        SpatialPredicate::Within,
        SpatialPredicate::Covers,
        SpatialPredicate::CoveredBy,
        SpatialPredicate::Touches,
        SpatialPredicate::Overlaps,
        SpatialPredicate::WithinDistance,
    ] {
        assert_eq!(SpatialPredicate::from_str(predicate.as_str()).unwrap(), predicate);
    }
}

#[test]
fn engine_new_replace_and_query_mask() {
    let engine = Engine::new(vec![rule("a", unit_square_geometry())]).unwrap();
    assert_eq!(engine.current().version, 1);

    let report = engine
        .replace(vec![rule(
            "b",
            Geometry::Polygon(square(100.0, 100.0, 110.0, 110.0)),
        )])
        .unwrap();
    assert_eq!(report.version, 2);
    assert_eq!(report.rule_count, 1);

    let candidates = vec![
        candidate("inside-a", square(2.0, 2.0, 4.0, 4.0)),
        candidate("inside-b", square(102.0, 102.0, 104.0, 104.0)),
    ];
    let mask = engine.query_mask(&candidates, &Query::new(SpatialPredicate::Intersects));
    assert_eq!(mask, vec![0, 1]);
}

#[test]
fn classify_candidate_returns_envelope_or_reason() {
    let valid = Geometry::Polygon(square(0.0, 0.0, 10.0, 10.0));
    assert_eq!(classify_candidate(&valid).unwrap(), Rect::new((0.0, 0.0), (10.0, 10.0)));

    let bowtie = Geometry::Polygon(bowtie());
    let error = classify_candidate(&bowtie).unwrap_err();
    assert_eq!(error.code, ErrorCode::InvalidGeometry);
    assert!(error.message.starts_with("invalid geometry:"));

    // Point and MultiPoint candidates are supported (filtering-scale 01).
    assert!(classify_candidate(&Geometry::Point(Point::new(1.0, 1.0))).is_ok());

    // LineString is not a supported candidate type.
    let line = Geometry::LineString(LineString::from(vec![(0.0, 0.0), (1.0, 1.0)]));
    let error = classify_candidate(&line).unwrap_err();
    assert_eq!(error.code, ErrorCode::UnsupportedGeometryType);
    assert_eq!(error.message, "unsupported geometry type: LineString");
}

#[test]
fn spatial_error_constructors_set_code_and_message() {
    let cases: Vec<(SpatialError, ErrorCode)> = vec![
        (SpatialError::invalid_geojson("m"), ErrorCode::InvalidGeoJson),
        (SpatialError::invalid_geometry("m"), ErrorCode::InvalidGeometry),
        (SpatialError::invalid_query("m"), ErrorCode::InvalidQuery),
        (
            SpatialError::invalid_property_predicate("m"),
            ErrorCode::InvalidPropertyPredicate,
        ),
        (
            SpatialError::unsupported_geometry_type("m"),
            ErrorCode::UnsupportedGeometryType,
        ),
        (
            SpatialError::unsupported_property_operator("m"),
            ErrorCode::UnsupportedPropertyOperator,
        ),
        (
            SpatialError::unsupported_spatial_predicate("m"),
            ErrorCode::UnsupportedSpatialPredicate,
        ),
    ];
    for (error, code) in cases {
        assert_eq!(error.code, code);
        assert_eq!(error.message, "m");
    }
    assert_eq!(SpatialError::new(ErrorCode::Native, "m").code, ErrorCode::Native);
}

#[test]
fn prepared_geometries_handle_is_indexed_by_rule_id() {
    use geo::Relate;

    let ruleset = Ruleset::from_geojson(TWO_RULES).unwrap();
    let prepared = ruleset.prepared();
    assert_eq!(prepared.len(), 2);
    assert!(!prepared.is_empty());
    assert_eq!(prepared.iter().count(), 2);

    let zone_a = ruleset.rule_id("zone-a").unwrap();
    let zone_b = ruleset.rule_id("zone-b").unwrap();
    // The handle returns the rule's own prepared geometry by opaque id: the
    // point (5, 5) is inside zone-a and disjoint from zone-b.
    let inside_a = Geometry::Point(Point::new(5.0, 5.0));
    assert!(inside_a
        .relate(prepared.get(zone_a).expect("minted by this ruleset"))
        .is_intersects());
    assert!(inside_a
        .relate(prepared.get(zone_b).expect("minted by this ruleset"))
        .is_disjoint());
}

#[test]
fn property_value_ordering_and_equality_are_typed() {
    use spatial_rules_core::PropertyValue;

    assert!(PropertyValue::Null < PropertyValue::Bool(false));
    assert!(PropertyValue::Int(1) < PropertyValue::Int(2));
    assert!(PropertyValue::Int(1) < PropertyValue::Str("a".to_string()));
    assert_eq!(PropertyValue::Float(1.0), PropertyValue::Float(1.0));
    // Different variants are never equal, even for the same numeric value.
    assert_ne!(PropertyValue::Int(1), PropertyValue::Float(1.0));
}
