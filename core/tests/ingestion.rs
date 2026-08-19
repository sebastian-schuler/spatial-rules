//! Integration tests for GeoJSON ingestion and the rule-geometry validity gate.
//!
//! These exercise the public API of `spatial-rules-core` at its seams:
//! parsing, feature → type conversion, and the validity gate. Expected values
//! are independent literals from the spec fixtures, not recomputed by the
//! code under test.

use geo::{LineString, Point, Polygon};
use spatial_rules_core::{
    candidate_from_feature, candidates_from_geojson, ensure_supported_geometry, feature_geometry,
    parse_geojson, rule_from_feature, rules_from_geojson, validate_rule_geometry, Candidate,
    ErrorCode, PropertyValue, Rule,
};

const VALID_COLLECTION: &str = r#"{
  "type": "FeatureCollection",
  "features": [
    {
      "type": "Feature",
      "id": "rule-17",
      "properties": {
        "name": "Example Zone",
        "active": true,
        "priority": 10,
        "score": 4.2,
        "note": null,
        "nested": { "ignored": true }
      },
      "geometry": {
        "type": "Polygon",
        "coordinates": [[[0, 0], [0, 10], [10, 10], [10, 0], [0, 0]]]
      }
    },
    {
      "type": "Feature",
      "properties": { "id": "rule-from-props" },
      "geometry": {
        "type": "MultiPolygon",
        "coordinates": [[[[0, 0], [0, 1], [1, 1], [1, 0], [0, 0]]]]
      }
    }
  ]
}"#;

fn square_polygon() -> Polygon<f64> {
    Polygon::new(
        LineString::from(vec![
            (0.0, 0.0),
            (0.0, 10.0),
            (10.0, 10.0),
            (10.0, 0.0),
            (0.0, 0.0),
        ]),
        vec![],
    )
}

#[test]
fn parses_a_valid_feature_collection() {
    let geojson = parse_geojson(VALID_COLLECTION).unwrap();
    match geojson {
        geojson::GeoJson::FeatureCollection(collection) => {
            assert_eq!(collection.features.len(), 2);
        }
        _ => panic!("expected a FeatureCollection"),
    }
}

#[test]
fn rejects_malformed_geojson_with_invalid_geojson() {
    let err = parse_geojson("not json").unwrap_err();
    assert_eq!(err.code, ErrorCode::InvalidGeoJson);
}

#[test]
fn rejects_non_feature_document() {
    let err = rules_from_geojson(r#"{"type": "Point", "coordinates": [0, 0]}"#).unwrap_err();
    assert_eq!(err.code, ErrorCode::InvalidGeoJson);
}

#[test]
fn extracts_polygon_geometry() {
    let features = rules_from_geojson(VALID_COLLECTION).unwrap();
    let expected = geo::Geometry::Polygon(square_polygon());
    assert_eq!(features[0].geometry, expected);
}

#[test]
fn builds_rule_with_typed_properties() {
    let rules = rules_from_geojson(VALID_COLLECTION).unwrap();
    let rule = &rules[0];

    assert_eq!(rule.id, "rule-17");
    assert_eq!(rule.properties.get("name"), Some(&PropertyValue::Str("Example Zone".into())));
    assert_eq!(rule.properties.get("active"), Some(&PropertyValue::Bool(true)));
    assert_eq!(rule.properties.get("priority"), Some(&PropertyValue::Int(10)));
    assert_eq!(rule.properties.get("score"), Some(&PropertyValue::Float(4.2)));
    assert_eq!(rule.properties.get("note"), Some(&PropertyValue::Null));
    assert!(!rule.properties.contains_key("nested"));
}

#[test]
fn falls_back_to_properties_id() {
    let rules = rules_from_geojson(VALID_COLLECTION).unwrap();
    assert_eq!(rules[1].id, "rule-from-props");
}

#[test]
fn rejects_feature_without_id() {
    let input = r#"{
      "type": "FeatureCollection",
      "features": [
        { "type": "Feature", "properties": {}, "geometry": null }
      ]
    }"#;
    let err = rules_from_geojson(input).unwrap_err();
    assert_eq!(err.code, ErrorCode::InvalidGeoJson);
}

#[test]
fn rejects_feature_without_geometry() {
    let input = r#"{
      "type": "FeatureCollection",
      "features": [
        { "type": "Feature", "id": "rule-1", "properties": {}, "geometry": null }
      ]
    }"#;
    let err = rules_from_geojson(input).unwrap_err();
    assert_eq!(err.code, ErrorCode::InvalidGeoJson);
}

#[test]
fn builds_candidate_from_feature() {
    let candidates: Vec<Candidate> = candidates_from_geojson(VALID_COLLECTION).unwrap();
    assert_eq!(candidates.len(), 2);
    assert_eq!(candidates[0].id, "rule-17");
    assert_eq!(candidates[0].geometry, geo::Geometry::Polygon(square_polygon()));
}

#[test]
fn single_feature_ingests_without_collection_wrapper() {
    let input = r#"{
      "type": "Feature",
      "id": "rule-1",
      "properties": {},
      "geometry": { "type": "Polygon", "coordinates": [[[0, 0], [0, 1], [1, 1], [1, 0], [0, 0]]] }
    }"#;
    let rules: Vec<Rule> = rules_from_geojson(input).unwrap();
    assert_eq!(rules.len(), 1);
    assert_eq!(rules[0].id, "rule-1");
}

#[test]
fn feature_geometry_rejects_missing_geometry() {
    let input = r#"{
      "type": "FeatureCollection",
      "features": [
        { "type": "Feature", "id": "rule-1", "properties": {}, "geometry": null }
      ]
    }"#;
    let collection = parse_geojson(input).unwrap();
    let geojson::GeoJson::FeatureCollection(collection) = collection else {
        panic!("expected a FeatureCollection");
    };
    let err = feature_geometry(&collection.features[0]).unwrap_err();
    assert_eq!(err.code, ErrorCode::InvalidGeoJson);
}

#[test]
fn accepts_valid_polygon_and_multipolygon() {
    let polygon = geo::Geometry::Polygon(square_polygon());
    validate_rule_geometry(&polygon).unwrap();

    let multipolygon = geo::Geometry::MultiPolygon(geo::MultiPolygon::new(vec![
        geo::Polygon::new(
            LineString::from(vec![
                (0.0, 0.0),
                (0.0, 1.0),
                (1.0, 1.0),
                (1.0, 0.0),
                (0.0, 0.0),
            ]),
            vec![],
        ),
    ]));
    validate_rule_geometry(&multipolygon).unwrap();
}

#[test]
fn accepts_polygon_with_a_hole() {
    let polygon = Polygon::new(
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
    );
    validate_rule_geometry(&geo::Geometry::Polygon(polygon)).unwrap();
}

#[test]
fn rejects_hole_outside_exterior() {
    // An interior ring that lies outside the exterior ring is OGC-invalid.
    let polygon = Polygon::new(
        LineString::from(vec![
            (0.0, 0.0),
            (0.0, 10.0),
            (10.0, 10.0),
            (10.0, 0.0),
            (0.0, 0.0),
        ]),
        vec![LineString::from(vec![
            (20.0, 20.0),
            (20.0, 30.0),
            (30.0, 30.0),
            (30.0, 20.0),
            (20.0, 20.0),
        ])],
    );
    let err = validate_rule_geometry(&geo::Geometry::Polygon(polygon)).unwrap_err();
    assert_eq!(err.code, ErrorCode::InvalidGeometry);
}

#[test]
fn rejects_non_finite_coordinate() {
    // NaN coordinates are malformed and must be rejected (§33).
    let polygon = Polygon::new(
        LineString::from(vec![
            (0.0, 0.0),
            (0.0, f64::NAN),
            (10.0, 10.0),
            (10.0, 0.0),
            (0.0, 0.0),
        ]),
        vec![],
    );
    let err = validate_rule_geometry(&geo::Geometry::Polygon(polygon)).unwrap_err();
    assert_eq!(err.code, ErrorCode::InvalidGeometry);
}

#[test]
fn rejects_self_intersecting_polygon_with_invalid_geometry() {
    // Bowtie: the two halves cross at the middle.
    let polygon = Polygon::new(
        LineString::from(vec![
            (0.0, 0.0),
            (10.0, 10.0),
            (0.0, 10.0),
            (10.0, 0.0),
            (0.0, 0.0),
        ]),
        vec![],
    );
    let err = validate_rule_geometry(&geo::Geometry::Polygon(polygon)).unwrap_err();
    assert_eq!(err.code, ErrorCode::InvalidGeometry);
}

#[test]
fn rejects_unsupported_geometry_type() {
    let point = geo::Geometry::Point(Point::new(0.0, 0.0));
    let err = ensure_supported_geometry(&point).unwrap_err();
    assert_eq!(err.code, ErrorCode::UnsupportedGeometryType);

    let err = validate_rule_geometry(&point).unwrap_err();
    assert_eq!(err.code, ErrorCode::UnsupportedGeometryType);
}

#[test]
fn rule_and_candidate_constructors_work_directly() {
    let rule = Rule {
        id: "rule-1".to_string(),
        properties: Default::default(),
        geometry: geo::Geometry::Polygon(square_polygon()),
    };
    let candidate = Candidate::new(
        "candidate-1".to_string(),
        geo::Geometry::Polygon(square_polygon()),
    );
    assert_eq!(rule.id, "rule-1");
    assert_eq!(candidate.id, "candidate-1");

    // The constructors accept a single feature directly.
    let input = r#"{
      "type": "Feature",
      "id": "rule-2",
      "properties": {},
      "geometry": { "type": "Polygon", "coordinates": [[[0, 0], [0, 1], [1, 1], [1, 0], [0, 0]]] }
    }"#;
    let feature = match parse_geojson(input).unwrap() {
        geojson::GeoJson::Feature(f) => f,
        _ => panic!("expected a Feature"),
    };
    assert!(rule_from_feature(&feature).is_ok());
    assert!(candidate_from_feature(&feature).is_ok());
}
