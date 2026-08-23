//! Integration tests for the `withinDistance` metric predicate (ADR-0016,
//! P2 ticket 03): minimum haversine distance admission (0 if inside), a
//! conservative bounding-circle pre-filter, strict query validation, and
//! resolution parity.

use spatial_rules_core::{
    candidates_from_geojson, ErrorCode, PropertyValue, Query, ResolutionOutcome, Rule, Ruleset,
    SpatialPredicate,
};

mod common;
use common::{candidate_geometry, rule_with_props, square};

/// The rule used across most cases: the unit square (0,0)-(1,1) (~111 km
/// across), so a point 0.001° north of its top edge is ~111 m away.
fn unit_square_rule(id: &str) -> Rule {
    rule_with_props(id, square(0.0, 0.0, 1.0, 1.0), &[])
}

fn distance_query(meters: f64) -> Query {
    Query::from_json(&serde_json::json!({
        "spatial": { "predicate": "withinDistance", "distance": meters }
    }))
    .unwrap()
}

fn point(lon: f64, lat: f64) -> spatial_rules_core::Candidate {
    candidate_geometry("p", geo::Geometry::Point(geo::Point::new(lon, lat)))
}

#[test]
fn inside_is_within_any_distance() {
    let ruleset = Ruleset::build(vec![unit_square_rule("zone")]).unwrap();
    // A point inside the rule is at distance 0, so within any positive radius.
    assert_eq!(
        ruleset.query_mask(&[point(0.5, 0.5)], &distance_query(1.0)),
        vec![1]
    );
}

#[test]
fn boundary_point_matches_at_a_tiny_distance() {
    let ruleset = Ruleset::build(vec![unit_square_rule("zone")]).unwrap();
    // A point exactly on the rule's edge is at a sub-10 m distance (geo's
    // haversine closest-point reports ~4 m for an exact-boundary point), so a
    // 10 m radius — tiny against the ~111 km rule — still admits it.
    assert_eq!(
        ruleset.query_mask(&[point(0.5, 1.0)], &distance_query(10.0)),
        vec![1]
    );
}

#[test]
fn close_point_matches_and_far_point_does_not() {
    let ruleset = Ruleset::build(vec![unit_square_rule("zone")]).unwrap();
    // ~0.001° ≈ 111 m north of the top edge: within 200 m, not within 50 m.
    let close = point(0.5, 1.001);
    assert_eq!(ruleset.query_mask(std::slice::from_ref(&close), &distance_query(200.0)), vec![1]);
    assert_eq!(ruleset.query_mask(std::slice::from_ref(&close), &distance_query(50.0)), vec![0]);

    // ~1° ≈ 111 km north: outside a 200 m radius.
    let far = point(0.5, 2.0);
    assert_eq!(ruleset.query_mask(&[far], &distance_query(200.0)), vec![0]);
}

#[test]
fn point_candidates_are_supported() {
    let ruleset = Ruleset::build(vec![unit_square_rule("zone")]).unwrap();
    let inside = point(0.5, 0.5);
    let outside = point(50.0, 50.0);
    let outcomes = ruleset.query(&[inside, outside], &distance_query(1000.0));
    assert!(matches!(
        &outcomes[0],
        spatial_rules_core::CandidateOutcome::Matched { rule_ids, .. }
            if rule_ids == &vec![ruleset.rule_id("zone").unwrap()]
    ));
    assert_eq!(outcomes[1], spatial_rules_core::CandidateOutcome::NotMatched);
}

#[test]
fn multipoint_matches_when_any_point_is_within() {
    let ruleset = Ruleset::build(vec![unit_square_rule("zone")]).unwrap();
    let multipoint = candidate_geometry(
        "m",
        geo::Geometry::MultiPoint(geo::MultiPoint::new(vec![
            geo::Point::new(50.0, 50.0),
            geo::Point::new(0.5, 0.5),
        ])),
    );
    assert_eq!(
        ruleset.query_mask(&[multipoint], &distance_query(100.0)),
        vec![1]
    );
}

#[test]
fn polygon_candidate_within_distance_is_invalid() {
    let ruleset = Ruleset::build(vec![unit_square_rule("zone")]).unwrap();
    let polygon = candidate_geometry(
        "poly",
        geo::Geometry::Polygon(square(0.2, 0.2, 0.8, 0.8)),
    );
    // v1 scope: withinDistance supports point/multipoint candidates (ADR-0016);
    // a polygon candidate is reported invalid rather than silently unmatched.
    let outcomes = ruleset.query(&[polygon], &distance_query(100.0));
    assert!(matches!(
        &outcomes[0],
        spatial_rules_core::CandidateOutcome::Invalid { reason }
            if reason.contains("withinDistance requires a point candidate")
    ));
}

#[test]
fn where_clause_and_exclusions_still_apply() {
    let ruleset = Ruleset::build(vec![
        {
            let mut rule = unit_square_rule("active-zone");
            rule.properties
                .insert("active".to_string(), PropertyValue::Bool(true));
            rule
        },
        {
            let mut rule = rule_with_props("inactive-zone", square(5.0, 5.0, 6.0, 6.0), &[]);
            rule.properties
                .insert("active".to_string(), PropertyValue::Bool(false));
            rule
        },
    ])
    .unwrap();
    let near = point(0.5, 1.001);

    // Both rules are within 200 m; a where clause keeps only the active one.
    let where_query = Query::from_json(&serde_json::json!({
        "spatial": { "predicate": "withinDistance", "distance": 200.0 },
        "where": { "active": true }
    }))
    .unwrap();
    let outcomes = ruleset.query(std::slice::from_ref(&near), &where_query);
    assert!(matches!(
        &outcomes[0],
        spatial_rules_core::CandidateOutcome::Matched { rule_ids, .. }
            if rule_ids == &vec![ruleset.rule_id("active-zone").unwrap()]
    ));

    // Excluding the active rule leaves none within range.
    let excluded = Query::from_json(&serde_json::json!({
        "spatial": { "predicate": "withinDistance", "distance": 200.0 },
        "excludeRuleIds": ["active-zone"]
    }))
    .unwrap();
    assert_eq!(
        ruleset.query_mask(std::slice::from_ref(&near), &excluded),
        vec![0]
    );
}

#[test]
fn distance_is_strictly_validated() {
    // withinDistance requires a distance.
    let err = Query::from_json(&serde_json::json!({
        "spatial": { "predicate": "withinDistance" }
    }))
    .unwrap_err();
    assert_eq!(err.code, ErrorCode::InvalidQuery);
    assert!(err.message.contains("distance"));

    // distance with a non-distance predicate is rejected.
    let err = Query::from_json(&serde_json::json!({
        "spatial": { "predicate": "intersects", "distance": 100.0 }
    }))
    .unwrap_err();
    assert_eq!(err.code, ErrorCode::InvalidQuery);

    // Zero, negative, and non-numeric distances are rejected.
    for bad in [0.0, -5.0, f64::NAN] {
        let err = Query::from_json(&serde_json::json!({
            "spatial": { "predicate": "withinDistance", "distance": bad }
        }))
        .unwrap_err();
        assert_eq!(err.code, ErrorCode::InvalidQuery, "distance {bad}");
    }
    let err = Query::from_json(&serde_json::json!({
        "spatial": { "predicate": "withinDistance", "distance": "close" }
    }))
    .unwrap_err();
    assert_eq!(err.code, ErrorCode::InvalidQuery);
}

#[test]
fn within_distance_feeds_resolution() {
    // Two overlapping-adjacent rules with distinct priorities; a point between
    // them resolves to the higher-priority one when both are within range.
    let ruleset = Ruleset::build(vec![
        {
            let mut rule = unit_square_rule("hi");
            rule.priority = 10;
            rule
        },
        {
            let mut rule = rule_with_props("lo", square(1.0, 0.0, 2.0, 1.0), &[]);
            rule.priority = 5;
            rule
        },
    ])
    .unwrap();
    // A point just east of the shared edge x=1 is within 200 m of both rules
    // ("lo" is inside — distance 0; "hi" is ~111 m east of its right edge).
    let candidates = candidates_from_geojson(r#"{
        "type": "FeatureCollection",
        "features": [
            { "type": "Feature", "id": "p", "properties": {}, "geometry": { "type": "Point", "coordinates": [1.001, 0.5] } }
        ]
    }"#)
    .unwrap();
    let outcomes = ruleset.resolve(&candidates, &distance_query(200.0));
    let ResolutionOutcome::Resolved {
        winner,
        applicable,
        ..
    } = &outcomes[0]
    else {
        panic!("expected a resolved outcome");
    };
    // Both are within range; the higher-priority rule wins.
    assert_eq!(*winner, ruleset.rule_id("hi").unwrap());
    assert_eq!(applicable.len(), 2);

    // A point 0.01° east is beyond 200 m of "hi" but still inside "lo".
    let candidates = candidates_from_geojson(r#"{
        "type": "FeatureCollection",
        "features": [
            { "type": "Feature", "id": "p", "properties": {}, "geometry": { "type": "Point", "coordinates": [1.01, 0.5] } }
        ]
    }"#)
    .unwrap();
    let outcomes = ruleset.resolve(&candidates, &distance_query(200.0));
    let ResolutionOutcome::Resolved {
        winner,
        applicable,
        ..
    } = &outcomes[0]
    else {
        panic!("expected a resolved outcome");
    };
    assert_eq!(*winner, ruleset.rule_id("lo").unwrap());
    assert_eq!(applicable.len(), 1);
}

#[test]
fn distance_mask_matches_distance_outcomes() {
    let ruleset = Ruleset::build(vec![unit_square_rule("zone")]).unwrap();
    let candidates = vec![
        point(0.5, 0.5),      // inside -> resolved
        point(0.5, 1.001),    // ~111 m -> resolved within 200 m
        point(0.5, 2.0),      // ~111 km -> not resolved
        candidate_geometry(
            "poly",
            geo::Geometry::Polygon(square(0.2, 0.2, 0.8, 0.8)),
        ), // polygon -> invalid for withinDistance
    ];
    let outcomes = ruleset.resolve(&candidates, &distance_query(200.0));
    let expected: Vec<u8> = outcomes
        .iter()
        .map(|outcome| match outcome {
            ResolutionOutcome::Resolved { .. } => 1,
            ResolutionOutcome::NotMatched => 0,
            ResolutionOutcome::Invalid { .. } => 2,
        })
        .collect();
    assert_eq!(ruleset.resolve_mask(&candidates, &distance_query(200.0)), expected);
}

#[test]
fn malformed_programmatic_distance_query_is_invalid_not_a_panic() {
    // The JSON parser validates `distance`, but a directly-constructed `Query`
    // must not panic in the evaluation path (structured-error model): a missing
    // or non-finite radius reports the candidate invalid instead.
    let ruleset = Ruleset::build(vec![unit_square_rule("zone")]).unwrap();
    let missing = Query::new(SpatialPredicate::WithinDistance);
    let outcomes = ruleset.query(&[point(0.5, 0.5)], &missing);
    assert!(matches!(
        &outcomes[0],
        spatial_rules_core::CandidateOutcome::Invalid { reason }
            if reason.contains("withinDistance requires a positive distance")
    ));

    let non_finite = Query::new(SpatialPredicate::WithinDistance).with_distance(f64::NAN);
    assert_eq!(
        ruleset.query_mask(&[point(0.5, 0.5)], &non_finite),
        vec![2]
    );
}

#[test]
fn plain_de9im_queries_are_unaffected() {
    let ruleset = Ruleset::build(vec![unit_square_rule("zone")]).unwrap();
    assert_eq!(
        ruleset.query_mask(&[point(0.5, 0.5)], &Query::new(SpatialPredicate::Intersects)),
        vec![1]
    );
    assert_eq!(
        ruleset.query_mask(&[point(50.0, 50.0)], &Query::new(SpatialPredicate::Intersects)),
        vec![0]
    );
}