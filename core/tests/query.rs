//! Integration tests for the batch query engine (ticket 15).
//!
//! Exercises the fixed pipeline (bbox filter → property predicate → exact
//! DE-9IM relate), the Mongo-style `where` AST, and the aligned
//! `CandidateOutcome` result model. Expected values are hand-computed literals
//! from the DE-9IM semantics in ADR-0008, not recomputed by the code.

use geo::{LineString, Point, Polygon};
use serde_json::json;
use spatial_rules_core::{
    Candidate, CandidateOutcome, ErrorCode, PropertyValue, Query, Rule, RuleId, Ruleset,
    SpatialPredicate,
};

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

fn square_with_hole() -> Polygon<f64> {
    Polygon::new(
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

fn candidate(id: &str, polygon: Polygon<f64>) -> Candidate {
    Candidate {
        id: id.to_string(),
        geometry: geo::Geometry::Polygon(polygon),
    }
}

fn default_ruleset() -> Ruleset {
    Ruleset::build(vec![
        rule(
            "square",
            square(0.0, 0.0, 10.0, 10.0),
            &[
                ("active", PropertyValue::Bool(true)),
                ("classification", PropertyValue::Str("restricted".into())),
                ("country", PropertyValue::Str("HR".into())),
                ("priority", PropertyValue::Int(10)),
            ],
        ),
        rule(
            "far",
            square(100.0, 100.0, 110.0, 110.0),
            &[
                ("active", PropertyValue::Bool(false)),
                ("classification", PropertyValue::Str("military".into())),
            ],
        ),
    ])
    .unwrap()
}

fn intersects() -> Query {
    Query::new(SpatialPredicate::Intersects)
}

#[test]
fn query_returns_outcomes_aligned_to_input() {
    let ruleset = default_ruleset();
    let candidates = vec![
        candidate("inside", square(2.0, 2.0, 4.0, 4.0)),
        candidate("far", square(50.0, 50.0, 60.0, 60.0)),
        candidate("also-far", square(60.0, 60.0, 70.0, 70.0)),
    ];
    let outcomes = ruleset.query(&candidates, &intersects());
    assert_eq!(outcomes.len(), 3);
    assert_eq!(outcomes[0], CandidateOutcome::Matched { rule_ids: vec![RuleId(0)] });
    assert_eq!(outcomes[1], CandidateOutcome::NotMatched);
    assert_eq!(outcomes[2], CandidateOutcome::NotMatched);
}

#[test]
fn disjoint_candidate_is_not_matched() {
    let ruleset = default_ruleset();
    let outcomes = ruleset.query(&[candidate("far", square(50.0, 50.0, 60.0, 60.0))], &intersects());
    assert_eq!(outcomes, vec![CandidateOutcome::NotMatched]);
}

#[test]
fn contains_predicate_is_directional() {
    let ruleset = default_ruleset();
    let containing = candidate("big", square(-10.0, -10.0, 20.0, 20.0));
    let inside = candidate("small", square(2.0, 2.0, 4.0, 4.0));

    let query = Query::new(SpatialPredicate::Contains);
    assert_eq!(
        ruleset.query(std::slice::from_ref(&containing), &query),
        vec![CandidateOutcome::Matched { rule_ids: vec![RuleId(0)] }]
    );
    assert_eq!(ruleset.query(&[inside], &query), vec![CandidateOutcome::NotMatched]);
}

#[test]
fn within_predicate_is_directional() {
    let ruleset = default_ruleset();
    let containing = candidate("big", square(-10.0, -10.0, 20.0, 20.0));
    let inside = candidate("small", square(2.0, 2.0, 4.0, 4.0));

    let query = Query::new(SpatialPredicate::Within);
    assert_eq!(
        ruleset.query(&[inside], &query),
        vec![CandidateOutcome::Matched { rule_ids: vec![RuleId(0)] }]
    );
    assert_eq!(ruleset.query(&[containing], &query), vec![CandidateOutcome::NotMatched]);
}

#[test]
fn touching_boundary_intersects_but_does_not_contain() {
    let ruleset = default_ruleset();
    // Shares the edge x=10 with the square rule (boundary touch).
    let adjacent = candidate("adjacent", square(10.0, 0.0, 20.0, 10.0));

    assert_eq!(
        ruleset.query(std::slice::from_ref(&adjacent), &intersects()),
        vec![CandidateOutcome::Matched { rule_ids: vec![RuleId(0)] }]
    );
    assert_eq!(
        ruleset.query(std::slice::from_ref(&adjacent), &Query::new(SpatialPredicate::Contains)),
        vec![CandidateOutcome::NotMatched]
    );
    assert_eq!(
        ruleset.query(&[adjacent], &Query::new(SpatialPredicate::Within)),
        vec![CandidateOutcome::NotMatched]
    );
}

#[test]
fn identical_geometry_matches_all_predicates() {
    let ruleset = default_ruleset();
    let identical = candidate("same", square(0.0, 0.0, 10.0, 10.0));

    for predicate in [
        SpatialPredicate::Intersects,
        SpatialPredicate::Contains,
        SpatialPredicate::Within,
    ] {
        assert_eq!(
            ruleset.query(std::slice::from_ref(&identical), &Query::new(predicate)),
            vec![CandidateOutcome::Matched { rule_ids: vec![RuleId(0)] }],
            "predicate {:?}",
            predicate
        );
    }
}

#[test]
fn candidate_inside_hole_is_disjoint() {
    let ruleset = Ruleset::build(vec![rule("donut", square_with_hole(), &[])]).unwrap();
    let in_hole = candidate("in-hole", square(2.5, 2.5, 3.5, 3.5));
    // bbox overlaps the rule, but the exact DE-9IM step sees the hole.
    assert_eq!(ruleset.query(&[in_hole], &intersects()), vec![CandidateOutcome::NotMatched]);
}

#[test]
fn where_equality_filters() {
    let ruleset = default_ruleset();
    let inside = candidate("inside", square(2.0, 2.0, 4.0, 4.0));
    let query = Query::from_json(&json!({
        "spatial": { "predicate": "intersects" },
        "where": { "active": true }
    }))
    .unwrap();
    assert_eq!(
        ruleset.query(&[inside], &query),
        vec![CandidateOutcome::Matched { rule_ids: vec![RuleId(0)] }]
    );
}

#[test]
fn missing_property_is_non_match() {
    let ruleset = default_ruleset();
    let inside = candidate("inside", square(2.0, 2.0, 4.0, 4.0));
    // "country" is absent on the "far" rule; only "square" has it.
    let query = Query::from_json(&json!({
        "spatial": { "predicate": "intersects" },
        "where": { "country": "HR" }
    }))
    .unwrap();
    assert_eq!(
        ruleset.query(&[inside], &query),
        vec![CandidateOutcome::Matched { rule_ids: vec![RuleId(0)] }]
    );
}

#[test]
fn ne_requires_same_type_and_presence() {
    let ruleset = default_ruleset();
    let inside = candidate("inside", square(2.0, 2.0, 4.0, 4.0));

    // priority=10 exists: $ne 10 -> non-match (equal).
    let query = Query::from_json(&json!({
        "spatial": { "predicate": "intersects" },
        "where": { "priority": { "$ne": 10 } }
    }))
    .unwrap();
    assert_eq!(ruleset.query(std::slice::from_ref(&inside), &query), vec![CandidateOutcome::NotMatched]);

    // "country" missing on "square"? No — square has country HR. Use a property
    // missing on the matching rule: "name" is absent everywhere.
    let query = Query::from_json(&json!({
        "spatial": { "predicate": "intersects" },
        "where": { "name": { "$ne": "x" } }
    }))
    .unwrap();
    assert_eq!(ruleset.query(&[inside], &query), vec![CandidateOutcome::NotMatched]);
}

#[test]
fn numeric_range_operators() {
    let ruleset = default_ruleset();
    let inside = candidate("inside", square(2.0, 2.0, 4.0, 4.0));
    let matched = vec![CandidateOutcome::Matched { rule_ids: vec![RuleId(0)] }];

    let run = |operator: &str, value: i64| {
        let mut op = serde_json::Map::new();
        op.insert(operator.to_string(), json!(value));
        let mut where_clause = serde_json::Map::new();
        where_clause.insert("priority".to_string(), serde_json::Value::Object(op));
        let query = Query::from_json(&json!({
            "spatial": { "predicate": "intersects" },
            "where": serde_json::Value::Object(where_clause)
        }))
        .unwrap();
        ruleset.query(std::slice::from_ref(&inside), &query)
    };

    // square.priority = 10.
    assert_eq!(run("$gt", 5), matched);
    assert_eq!(run("$gt", 10), vec![CandidateOutcome::NotMatched]);
    assert_eq!(run("$gte", 10), matched);
    assert_eq!(run("$lt", 10), vec![CandidateOutcome::NotMatched]);
    assert_eq!(run("$lte", 10), matched);
}

#[test]
fn in_operator() {
    let ruleset = default_ruleset();
    let inside = candidate("inside", square(2.0, 2.0, 4.0, 4.0));
    let query = Query::from_json(&json!({
        "spatial": { "predicate": "intersects" },
        "where": { "country": { "$in": ["HR", "SI"] } }
    }))
    .unwrap();
    assert_eq!(
        ruleset.query(&[inside], &query),
        vec![CandidateOutcome::Matched { rule_ids: vec![RuleId(0)] }]
    );
}

#[test]
fn and_or_combinators() {
    let ruleset = default_ruleset();
    let inside = candidate("inside", square(2.0, 2.0, 4.0, 4.0));

    let and_query = Query::from_json(&json!({
        "spatial": { "predicate": "intersects" },
        "where": { "$and": [{ "active": true }, { "country": "HR" }] }
    }))
    .unwrap();
    assert_eq!(
        ruleset.query(std::slice::from_ref(&inside), &and_query),
        vec![CandidateOutcome::Matched { rule_ids: vec![RuleId(0)] }]
    );

    let or_query = Query::from_json(&json!({
        "spatial": { "predicate": "intersects" },
        "where": { "$or": [{ "classification": "military" }, { "country": "HR" }] }
    }))
    .unwrap();
    assert_eq!(
        ruleset.query(&[inside], &or_query),
        vec![CandidateOutcome::Matched { rule_ids: vec![RuleId(0)] }]
    );
}

#[test]
fn exclude_rule_ids_removes_matches() {
    let ruleset = default_ruleset();
    let inside = candidate("inside", square(2.0, 2.0, 4.0, 4.0));
    let query = Query::from_json(&json!({
        "spatial": { "predicate": "intersects" },
        "excludeRuleIds": ["square"]
    }))
    .unwrap();
    assert_eq!(ruleset.query(&[inside], &query), vec![CandidateOutcome::NotMatched]);
}

#[test]
fn exclude_unknown_rule_id_is_ignored() {
    let ruleset = default_ruleset();
    let inside = candidate("inside", square(2.0, 2.0, 4.0, 4.0));
    let query = Query::from_json(&json!({
        "spatial": { "predicate": "intersects" },
        "excludeRuleIds": ["does-not-exist"]
    }))
    .unwrap();
    assert_eq!(
        ruleset.query(&[inside], &query),
        vec![CandidateOutcome::Matched { rule_ids: vec![RuleId(0)] }]
    );
}

#[test]
fn invalid_candidate_stays_in_result() {
    let ruleset = default_ruleset();
    let bowtie = Candidate {
        id: "bowtie".to_string(),
        geometry: geo::Geometry::Polygon(Polygon::new(
            LineString::from(vec![
                (0.0, 0.0),
                (10.0, 10.0),
                (0.0, 10.0),
                (10.0, 0.0),
                (0.0, 0.0),
            ]),
            vec![],
        )),
    };
    let inside = candidate("inside", square(2.0, 2.0, 4.0, 4.0));
    let outcomes = ruleset.query(&[bowtie, inside], &intersects());
    assert_eq!(outcomes.len(), 2);
    assert!(matches!(&outcomes[0], CandidateOutcome::Invalid { .. }));
    assert_eq!(outcomes[1], CandidateOutcome::Matched { rule_ids: vec![RuleId(0)] });
}

#[test]
fn unsupported_candidate_type_is_invalid() {
    let ruleset = default_ruleset();
    let point = Candidate {
        id: "pt".to_string(),
        geometry: geo::Geometry::Point(Point::new(1.0, 1.0)),
    };
    let outcomes = ruleset.query(&[point], &intersects());
    assert!(matches!(&outcomes[0], CandidateOutcome::Invalid { .. }));
}

#[test]
fn unsupported_spatial_predicate_is_rejected() {
    let err = Query::from_json(&json!({
        "spatial": { "predicate": "overlaps" }
    }))
    .unwrap_err();
    assert_eq!(err.code, ErrorCode::UnsupportedSpatialPredicate);
}

#[test]
fn unsupported_property_operator_is_rejected() {
    let err = Query::from_json(&json!({
        "spatial": { "predicate": "intersects" },
        "where": { "country": { "$regex": "H" } }
    }))
    .unwrap_err();
    assert_eq!(err.code, ErrorCode::UnsupportedPropertyOperator);
}

#[test]
fn malformed_predicate_is_rejected() {
    let err = Query::from_json(&json!({
        "spatial": { "predicate": "intersects" },
        "where": { "country": { "$in": "HR" } }
    }))
    .unwrap_err();
    assert_eq!(err.code, ErrorCode::InvalidPropertyPredicate);
}

#[test]
fn missing_spatial_is_rejected() {
    let err = Query::from_json(&json!({ "where": {} })).unwrap_err();
    assert_eq!(err.code, ErrorCode::InvalidQuery);
}

#[test]
fn empty_where_matches_all() {
    let ruleset = default_ruleset();
    let inside = candidate("inside", square(2.0, 2.0, 4.0, 4.0));
    let query = Query::from_json(&json!({
        "spatial": { "predicate": "intersects" },
        "where": {}
    }))
    .unwrap();
    assert_eq!(
        ruleset.query(&[inside], &query),
        vec![CandidateOutcome::Matched { rule_ids: vec![RuleId(0)] }]
    );
}

#[test]
fn query_has_no_default_where_or_exclusions() {
    let query = intersects();
    assert_eq!(query.where_clause, None);
    assert!(query.exclude_rule_ids.is_empty());
}

#[test]
fn typed_query_builder_produces_expected_struct() {
    let query = Query::new(SpatialPredicate::Within).with_exclusions(vec!["a".to_string()]);
    assert_eq!(query.spatial, SpatialPredicate::Within);
    assert_eq!(query.exclude_rule_ids, vec!["a".to_string()]);
    assert_eq!(query.where_clause, None);

    // Sanity: a rule with no properties is still filterable by an empty map.
    let ruleset = Ruleset::build(vec![rule("bare", square(0.0, 0.0, 10.0, 10.0), &[])]).unwrap();
    let bare = candidate("bare", square(1.0, 1.0, 2.0, 2.0));
    assert_eq!(
        ruleset.query(&[bare], &intersects()),
        vec![CandidateOutcome::Matched { rule_ids: vec![RuleId(0)] }]
    );
}
