//! Error-model matrix (ticket 07): every stable `SR_*` code is reachable by a
//! documented input, so the error surface is explicit and enforceable.

use geo::Point;
use serde_json::json;
use spatial_rules_core::{ErrorCode, Query, Ruleset, SpatialError};

mod common;
use common::{bowtie, rule, square};

/// `SR_NATIVE` is intentionally absent: it is the reserved catch-all for
/// unexpected runtime failures and has no deterministic public input.
#[test]
fn every_error_code_is_reachable_by_a_documented_input() {
    let cases: Vec<(ErrorCode, SpatialError)> = vec![
        (
            ErrorCode::InvalidGeoJson,
            Ruleset::from_geojson("not json").unwrap_err(),
        ),
        (
            ErrorCode::InvalidGeometry,
            Ruleset::build(vec![rule("bad", geo::Geometry::Polygon(bowtie()))]).unwrap_err(),
        ),
        (
            ErrorCode::InvalidQuery,
            Query::from_json(&json!({ "where": {} })).unwrap_err(),
        ),
        (
            ErrorCode::InvalidPropertyPredicate,
            Query::from_json(&json!({
                "spatial": { "predicate": "intersects" },
                "where": { "x": { "$in": "not-an-array" } }
            }))
            .unwrap_err(),
        ),
        (
            ErrorCode::RulesetConstructionFailed,
            Ruleset::build(vec![
                rule("a", geo::Geometry::Polygon(square(0.0, 0.0, 10.0, 10.0))),
                rule("a", geo::Geometry::Polygon(square(0.0, 0.0, 10.0, 10.0))),
            ])
            .unwrap_err(),
        ),
        (
            ErrorCode::UnsupportedGeometryType,
            Ruleset::build(vec![rule(
                "point",
                geo::Geometry::Point(Point::new(1.0, 1.0)),
            )])
            .unwrap_err(),
        ),
        (
            ErrorCode::UnsupportedSpatialPredicate,
            Query::from_json(&json!({ "spatial": { "predicate": "crosses" } })).unwrap_err(),
        ),
        (
            ErrorCode::UnsupportedPropertyOperator,
            Query::from_json(&json!({
                "spatial": { "predicate": "intersects" },
                "where": { "x": { "$regex": "H" } }
            }))
            .unwrap_err(),
        ),
    ];

    let covered: Vec<ErrorCode> = cases
        .iter()
        .map(|(expected, actual)| {
            assert_eq!(&actual.code, expected, "wrong code for {expected:?}");
            *expected
        })
        .collect();

    // Every non-reserved code is covered by at least one case.
    for code in [
        ErrorCode::InvalidGeoJson,
        ErrorCode::InvalidGeometry,
        ErrorCode::InvalidQuery,
        ErrorCode::InvalidPropertyPredicate,
        ErrorCode::RulesetConstructionFailed,
        ErrorCode::UnsupportedGeometryType,
        ErrorCode::UnsupportedSpatialPredicate,
        ErrorCode::UnsupportedPropertyOperator,
    ] {
        assert!(covered.contains(&code), "no input reaches {code:?}");
    }
}

#[test]
fn native_code_is_reserved_and_renders_stably() {
    assert_eq!(ErrorCode::Native.as_str(), "SR_NATIVE");
}
