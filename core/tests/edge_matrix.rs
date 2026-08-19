//! Edge/input matrix (ticket 07): degenerate and hostile inputs map to the
//! documented outcome or `SR_*` error — and never panic.

use geo::{Geometry, LineString, Point, Polygon};
use spatial_rules_core::{candidates_from_geojson, Query, Ruleset, SpatialPredicate};

mod common;
use common::{candidate_geometry, rule, square, unit_square_geometry};

fn single_rule_ruleset() -> Ruleset {
    Ruleset::build(vec![rule("zone", unit_square_geometry())]).unwrap()
}

#[test]
fn empty_feature_collection_builds_empty_ruleset() {
    let ruleset = Ruleset::from_geojson(r#"{ "type": "FeatureCollection", "features": [] }"#).unwrap();
    assert!(ruleset.is_empty());
    assert_eq!(ruleset.len(), 0);
}

#[test]
fn empty_candidates_yield_empty_mask() {
    let ruleset = single_rule_ruleset();
    let candidates = candidates_from_geojson(r#"{ "type": "FeatureCollection", "features": [] }"#).unwrap();
    assert_eq!(
        ruleset.query_mask(&candidates, &Query::new(SpatialPredicate::Intersects)),
        Vec::<u8>::new()
    );
}

#[test]
fn feature_without_id_is_rejected() {
    let err = Ruleset::from_geojson(
        r#"{ "type": "FeatureCollection", "features": [ { "type": "Feature", "properties": {}, "geometry": { "type": "Polygon", "coordinates": [[[0,0],[0,1],[1,1],[1,0],[0,0]]] } } ] }"#,
    )
    .unwrap_err();
    assert_eq!(err.code, spatial_rules_core::ErrorCode::InvalidGeoJson);
}

#[test]
fn utf8_bom_prefix_is_rejected() {
    // A UTF-8 BOM before valid JSON is malformed JSON, not a silent parse.
    let err = Ruleset::from_geojson("\u{feff}{ \"type\": \"FeatureCollection\", \"features\": [] }")
        .unwrap_err();
    assert_eq!(err.code, spatial_rules_core::ErrorCode::InvalidGeoJson);
}

#[test]
fn non_finite_candidate_is_invalid_not_panic() {
    let ruleset = single_rule_ruleset();
    let nan_candidate = candidate_geometry(
        "nan",
        Geometry::Polygon(Polygon::new(
            LineString::from(vec![
                (0.0, 0.0),
                (0.0, f64::NAN),
                (1.0, 1.0),
                (1.0, 0.0),
                (0.0, 0.0),
            ]),
            vec![],
        )),
    );
    let outcomes = ruleset.query(
        std::slice::from_ref(&nan_candidate),
        &Query::new(SpatialPredicate::Intersects),
    );
    assert!(matches!(
        &outcomes[0],
        spatial_rules_core::CandidateOutcome::Invalid { .. }
    ));
    assert_eq!(
        ruleset.query_mask(
            std::slice::from_ref(&nan_candidate),
            &Query::new(SpatialPredicate::Intersects)
        ),
        vec![2]
    );
}

#[test]
fn antimeridian_crossing_rule_queries_without_panic() {
    // A rule crossing lon ±180 and a candidate inside it: valid, and the
    // engine treats longitude as an ordinary coordinate.
    let ruleset = Ruleset::build(vec![spatial_rules_core::Rule {
        id: "antimeridian".to_string(),
        properties: Default::default(),
        geometry: Geometry::Polygon(Polygon::new(
            LineString::from(vec![
                (179.0, -1.0),
                (179.0, 1.0),
                (181.0, 1.0),
                (181.0, -1.0),
                (179.0, -1.0),
            ]),
            vec![],
        )),
    }])
    .unwrap();
    let candidate = candidate_geometry(
        "inside",
        Geometry::Polygon(square(179.5, -0.5, 180.5, 0.5)),
    );
    let mask = ruleset.query_mask(std::slice::from_ref(&candidate), &Query::new(SpatialPredicate::Intersects));
    assert_eq!(mask, vec![1]);
}

#[test]
fn unsupported_rule_geometry_type_is_rejected() {
    let err = Ruleset::build(vec![spatial_rules_core::Rule {
        id: "point".to_string(),
        properties: Default::default(),
        geometry: Geometry::Point(Point::new(1.0, 1.0)),
    }])
    .unwrap_err();
    assert_eq!(err.code, spatial_rules_core::ErrorCode::UnsupportedGeometryType);
}

#[test]
fn array_and_object_property_values_are_skipped() {
    // Nested objects/arrays are not stored (ADR-0003); the rule still builds
    // and its scalar properties remain queryable.
    let ruleset = Ruleset::from_geojson(
        r#"{
            "type": "FeatureCollection",
            "features": [{
                "type": "Feature",
                "id": "zone",
                "properties": { "active": true, "nested": { "a": 1 }, "list": [1, 2] },
                "geometry": { "type": "Polygon", "coordinates": [[[0,0],[0,10],[10,10],[10,0],[0,0]]] }
            }]
        }"#,
    )
    .unwrap();
    let zone = ruleset.rule_id("zone").unwrap();
    assert_eq!(
        ruleset.properties(zone).get("active"),
        Some(&spatial_rules_core::PropertyValue::Bool(true))
    );
    assert!(!ruleset.properties(zone).contains_key("nested"));
    assert!(!ruleset.properties(zone).contains_key("list"));
}
