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
    SpatialError, SpatialPredicate,
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

fn square_id(ruleset: &Ruleset) -> RuleId {
    ruleset.rule_id("square").unwrap()
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
    assert_eq!(outcomes[0], CandidateOutcome::Matched { rule_ids: vec![square_id(&ruleset)] });
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
        vec![CandidateOutcome::Matched { rule_ids: vec![square_id(&ruleset)] }]
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
        vec![CandidateOutcome::Matched { rule_ids: vec![square_id(&ruleset)] }]
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
        vec![CandidateOutcome::Matched { rule_ids: vec![square_id(&ruleset)] }]
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
        SpatialPredicate::Covers,
        SpatialPredicate::CoveredBy,
    ] {
        assert_eq!(
            ruleset.query(std::slice::from_ref(&identical), &Query::new(predicate)),
            vec![CandidateOutcome::Matched { rule_ids: vec![square_id(&ruleset)] }],
            "predicate {:?}",
            predicate
        );
    }
}

#[test]
fn covers_predicate_is_directional() {
    let ruleset = default_ruleset();
    let big = candidate("big", square(-10.0, -10.0, 20.0, 20.0));
    let inside = candidate("small", square(2.0, 2.0, 4.0, 4.0));
    let query = Query::new(SpatialPredicate::Covers);

    // big covers the square rule.
    assert_eq!(
        ruleset.query(std::slice::from_ref(&big), &query),
        vec![CandidateOutcome::Matched { rule_ids: vec![square_id(&ruleset)] }]
    );
    // small does not cover the rule.
    assert_eq!(ruleset.query(&[inside], &query), vec![CandidateOutcome::NotMatched]);
}

#[test]
fn covered_by_predicate_is_directional() {
    let ruleset = default_ruleset();
    let big = candidate("big", square(-10.0, -10.0, 20.0, 20.0));
    let inside = candidate("small", square(2.0, 2.0, 4.0, 4.0));
    let query = Query::new(SpatialPredicate::CoveredBy);

    // small is covered by the square rule.
    assert_eq!(
        ruleset.query(std::slice::from_ref(&inside), &query),
        vec![CandidateOutcome::Matched { rule_ids: vec![square_id(&ruleset)] }]
    );
    // big is not covered by the rule.
    assert_eq!(ruleset.query(&[big], &query), vec![CandidateOutcome::NotMatched]);
}

#[test]
fn touches_true_on_shared_boundary() {
    let ruleset = default_ruleset();
    let query = Query::new(SpatialPredicate::Touches);

    // Shares the edge x=10 with the square rule (boundary touch, no interior
    // overlap).
    let adjacent = candidate("adjacent", square(10.0, 0.0, 20.0, 10.0));
    assert_eq!(
        ruleset.query(std::slice::from_ref(&adjacent), &query),
        vec![CandidateOutcome::Matched { rule_ids: vec![square_id(&ruleset)] }]
    );
    // Fully inside does not touch.
    let inside = candidate("inside", square(2.0, 2.0, 4.0, 4.0));
    assert_eq!(ruleset.query(&[inside], &query), vec![CandidateOutcome::NotMatched]);
    // Disjoint does not touch.
    let far = candidate("far", square(50.0, 50.0, 60.0, 60.0));
    assert_eq!(ruleset.query(&[far], &query), vec![CandidateOutcome::NotMatched]);
}

#[test]
fn overlaps_only_for_same_dimension_interior_overlap() {
    let ruleset = default_ruleset();
    let query = Query::new(SpatialPredicate::Overlaps);

    // Partial interior overlap with the square rule (0,0)-(10,10).
    let partial = candidate("partial", square(5.0, 5.0, 15.0, 15.0));
    assert_eq!(
        ruleset.query(std::slice::from_ref(&partial), &query),
        vec![CandidateOutcome::Matched { rule_ids: vec![square_id(&ruleset)] }]
    );

    // Full containment does not overlap.
    let inside = candidate("inside", square(2.0, 2.0, 4.0, 4.0));
    assert_eq!(ruleset.query(&[inside], &query), vec![CandidateOutcome::NotMatched]);

    // Covering the whole rule does not overlap.
    let big = candidate("big", square(-10.0, -10.0, 20.0, 20.0));
    assert_eq!(ruleset.query(&[big], &query), vec![CandidateOutcome::NotMatched]);

    // Boundary touch does not overlap.
    let adjacent = candidate("adjacent", square(10.0, 0.0, 20.0, 10.0));
    assert_eq!(ruleset.query(&[adjacent], &query), vec![CandidateOutcome::NotMatched]);
}

#[test]
fn new_predicates_parse_from_string() {
    use std::str::FromStr;
    for (s, expected) in [
        ("covers", SpatialPredicate::Covers),
        ("covered_by", SpatialPredicate::CoveredBy),
        ("touches", SpatialPredicate::Touches),
        ("overlaps", SpatialPredicate::Overlaps),
    ] {
        assert_eq!(SpatialPredicate::from_str(s).unwrap(), expected);
        assert_eq!(expected.as_str(), s);
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
        vec![CandidateOutcome::Matched { rule_ids: vec![square_id(&ruleset)] }]
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
        vec![CandidateOutcome::Matched { rule_ids: vec![square_id(&ruleset)] }]
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
    let matched = vec![CandidateOutcome::Matched { rule_ids: vec![square_id(&ruleset)] }];

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
        vec![CandidateOutcome::Matched { rule_ids: vec![square_id(&ruleset)] }]
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
        vec![CandidateOutcome::Matched { rule_ids: vec![square_id(&ruleset)] }]
    );

    let or_query = Query::from_json(&json!({
        "spatial": { "predicate": "intersects" },
        "where": { "$or": [{ "classification": "military" }, { "country": "HR" }] }
    }))
    .unwrap();
    assert_eq!(
        ruleset.query(&[inside], &or_query),
        vec![CandidateOutcome::Matched { rule_ids: vec![square_id(&ruleset)] }]
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
        vec![CandidateOutcome::Matched { rule_ids: vec![square_id(&ruleset)] }]
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
    assert_eq!(outcomes[1], CandidateOutcome::Matched { rule_ids: vec![square_id(&ruleset)] });
}

#[test]
fn unsupported_candidate_type_is_invalid() {
    let ruleset = default_ruleset();
    let point = Candidate {
        id: "pt".to_string(),
        geometry: geo::Geometry::Point(Point::new(1.0, 1.0)),
    };
    let outcomes = ruleset.query(&[point], &intersects());
    assert_eq!(
        outcomes[0],
        CandidateOutcome::Invalid {
            reason: "unsupported geometry type: Point".to_string()
        }
    );
}

#[test]
fn invalid_geometry_reason_is_stable() {
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
    let outcomes = ruleset.query(&[bowtie], &intersects());
    let CandidateOutcome::Invalid { reason } = &outcomes[0] else {
        panic!("expected an invalid outcome");
    };
    assert!(reason.starts_with("invalid geometry:"));
}

#[test]
fn candidate_matching_multiple_rules_reports_all_ids() {
    let ruleset = Ruleset::build(vec![
        rule("a", square(0.0, 0.0, 10.0, 10.0), &[]),
        rule("b", square(5.0, 5.0, 15.0, 15.0), &[]),
    ])
    .unwrap();
    let both = candidate("both", square(6.0, 6.0, 8.0, 8.0));
    let outcomes = ruleset.query(&[both], &intersects());
    let CandidateOutcome::Matched { rule_ids } = &outcomes[0] else {
        panic!("expected a match");
    };
    assert_eq!(
        rule_ids,
        &[ruleset.rule_id("a").unwrap(), ruleset.rule_id("b").unwrap()]
    );
}

#[test]
fn unsupported_spatial_predicate_is_rejected() {
    let err = Query::from_json(&json!({
        "spatial": { "predicate": "crosses" }
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
fn prepared_query_evaluates_and_collects_ids() {
    let ruleset = default_ruleset();
    let inside = candidate("inside", square(2.0, 2.0, 4.0, 4.0));
    let far = candidate("far", square(50.0, 50.0, 60.0, 60.0));

    let prepared = ruleset.prepare(&intersects());

    assert_eq!(
        prepared.evaluate(&inside),
        CandidateOutcome::Matched { rule_ids: vec![square_id(&ruleset)] }
    );
    assert_eq!(prepared.evaluate(&far), CandidateOutcome::NotMatched);
    assert_eq!(prepared.evaluate_mask(&inside), 1);
    assert_eq!(prepared.evaluate_mask(&far), 0);
}

#[test]
fn prepared_query_reports_invalid_candidate() {
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
    let prepared = ruleset.prepare(&intersects());
    assert!(matches!(
        prepared.evaluate(&bowtie),
        CandidateOutcome::Invalid { .. }
    ));
    assert_eq!(prepared.evaluate_mask(&bowtie), 2);
}

#[test]
fn prepared_query_applies_where_and_exclusions() {
    let ruleset = default_ruleset();
    let inside = candidate("inside", square(2.0, 2.0, 4.0, 4.0));

    let query = Query::from_json(&json!({
        "spatial": { "predicate": "intersects" },
        "where": { "active": false }
    }))
    .unwrap();
    assert_eq!(
        ruleset.prepare(&query).evaluate(&inside),
        CandidateOutcome::NotMatched
    );

    let excluded = Query::from_json(&json!({
        "spatial": { "predicate": "intersects" },
        "excludeRuleIds": ["square"]
    }))
    .unwrap();
    assert_eq!(
        ruleset.prepare(&excluded).evaluate(&inside),
        CandidateOutcome::NotMatched
    );
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
        vec![CandidateOutcome::Matched { rule_ids: vec![square_id(&ruleset)] }]
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
        vec![CandidateOutcome::Matched { rule_ids: vec![ruleset.rule_id("bare").unwrap()] }]
    );
}

// --- Ticket 01: richer where operators $not / $nin / $exists (ADR-0011) ---

fn where_query(where_clause: serde_json::Value) -> Query {
    Query::from_json(&json!({
        "spatial": { "predicate": "intersects" },
        "where": where_clause
    }))
    .unwrap()
}

#[test]
fn nin_excludes_listed_values() {
    let ruleset = default_ruleset();
    let inside = candidate("inside", square(2.0, 2.0, 4.0, 4.0));
    let matched = vec![CandidateOutcome::Matched { rule_ids: vec![square_id(&ruleset)] }];

    // square.country = "HR": not in ["SI", "DE"] -> match.
    assert_eq!(
        ruleset.query(std::slice::from_ref(&inside), &where_query(json!({ "country": { "$nin": ["SI", "DE"] } }))),
        matched
    );
    // square.country = "HR": in ["HR"] -> non-match.
    assert_eq!(
        ruleset.query(std::slice::from_ref(&inside), &where_query(json!({ "country": { "$nin": ["HR"] } }))),
        vec![CandidateOutcome::NotMatched]
    );
}

#[test]
fn nin_missing_field_is_non_match() {
    let ruleset = default_ruleset();
    let inside = candidate("inside", square(2.0, 2.0, 4.0, 4.0));
    // "name" is absent everywhere: documented divergence from Mongo -> non-match.
    assert_eq!(
        ruleset.query(std::slice::from_ref(&inside), &where_query(json!({ "name": { "$nin": ["x"] } }))),
        vec![CandidateOutcome::NotMatched]
    );
}

#[test]
fn nin_type_mismatch_is_non_match() {
    let ruleset = default_ruleset();
    let inside = candidate("inside", square(2.0, 2.0, 4.0, 4.0));
    // square.country is Str but the list holds Ints: type mismatch -> non-match.
    assert_eq!(
        ruleset.query(std::slice::from_ref(&inside), &where_query(json!({ "country": { "$nin": [10] } }))),
        vec![CandidateOutcome::NotMatched]
    );
}

#[test]
fn exists_checks_presence() {
    let ruleset = default_ruleset();
    let inside = candidate("inside", square(2.0, 2.0, 4.0, 4.0));
    let matched = vec![CandidateOutcome::Matched { rule_ids: vec![square_id(&ruleset)] }];

    // square has "active" and "country"; "name" is absent.
    assert_eq!(
        ruleset.query(std::slice::from_ref(&inside), &where_query(json!({ "active": { "$exists": true } }))),
        matched
    );
    assert_eq!(
        ruleset.query(std::slice::from_ref(&inside), &where_query(json!({ "active": { "$exists": false } }))),
        vec![CandidateOutcome::NotMatched]
    );
    assert_eq!(
        ruleset.query(std::slice::from_ref(&inside), &where_query(json!({ "name": { "$exists": true } }))),
        vec![CandidateOutcome::NotMatched]
    );
    assert_eq!(
        ruleset.query(std::slice::from_ref(&inside), &where_query(json!({ "name": { "$exists": false } }))),
        matched
    );
}

#[test]
fn not_negates_inner_predicate() {
    let ruleset = default_ruleset();
    let inside = candidate("inside", square(2.0, 2.0, 4.0, 4.0));
    let matched = vec![CandidateOutcome::Matched { rule_ids: vec![square_id(&ruleset)] }];

    // square.active = true: $eq true -> match, so $not -> non-match.
    assert_eq!(
        ruleset.query(std::slice::from_ref(&inside), &where_query(json!({ "active": { "$not": { "$eq": true } } }))),
        vec![CandidateOutcome::NotMatched]
    );
    // square.active != false, so $not { $eq: false } -> match.
    assert_eq!(
        ruleset.query(std::slice::from_ref(&inside), &where_query(json!({ "active": { "$not": { "$eq": false } } }))),
        matched
    );
}

#[test]
fn not_negates_inner_on_missing_field() {
    let ruleset = default_ruleset();
    let inside = candidate("inside", square(2.0, 2.0, 4.0, 4.0));
    // "name" is missing, so the inner $eq is a non-match; $not negates it to a match.
    assert_eq!(
        ruleset.query(std::slice::from_ref(&inside), &where_query(json!({ "name": { "$not": { "$eq": "x" } } }))),
        vec![CandidateOutcome::Matched { rule_ids: vec![square_id(&ruleset)] }]
    );
}

#[test]
fn nested_not_double_negates() {
    let ruleset = default_ruleset();
    let inside = candidate("inside", square(2.0, 2.0, 4.0, 4.0));
    // $not($not($eq true)) collapses to $eq true: square.active = true -> match.
    assert_eq!(
        ruleset.query(std::slice::from_ref(&inside), &where_query(json!({ "active": { "$not": { "$not": { "$eq": true } } } }))),
        vec![CandidateOutcome::Matched { rule_ids: vec![square_id(&ruleset)] }]
    );
}

#[test]
fn not_parity_with_ne() {
    let ruleset = default_ruleset();
    let inside = candidate("inside", square(2.0, 2.0, 4.0, 4.0));
    let matched = vec![CandidateOutcome::Matched { rule_ids: vec![square_id(&ruleset)] }];

    // $not { $ne: "HR" } behaves like equality for a present, same-typed field.
    assert_eq!(
        ruleset.query(std::slice::from_ref(&inside), &where_query(json!({ "country": { "$not": { "$ne": "HR" } } }))),
        matched
    );
    assert_eq!(
        ruleset.query(std::slice::from_ref(&inside), &where_query(json!({ "country": { "$not": { "$ne": "SI" } } }))),
        vec![CandidateOutcome::NotMatched]
    );
}

#[test]
fn new_operators_compose_inside_and_or() {
    let ruleset = default_ruleset();
    let inside = candidate("inside", square(2.0, 2.0, 4.0, 4.0));
    let matched = vec![CandidateOutcome::Matched { rule_ids: vec![square_id(&ruleset)] }];

    let and_query = where_query(json!({
        "$and": [
            { "country": { "$nin": ["SI", "DE"] } },
            { "active": { "$exists": true } }
        ]
    }));
    assert_eq!(ruleset.query(std::slice::from_ref(&inside), &and_query), matched);

    let or_query = where_query(json!({
        "$or": [
            { "country": { "$nin": ["HR"] } },
            { "active": { "$not": { "$eq": true } } }
        ]
    }));
    assert_eq!(ruleset.query(std::slice::from_ref(&inside), &or_query), vec![CandidateOutcome::NotMatched]);
}

#[test]
fn malformed_new_operators_are_rejected() {
    // $exists requires a boolean operand.
    let err = where_query_err(json!({ "active": { "$exists": "yes" } }));
    assert_eq!(err.code, ErrorCode::InvalidPropertyPredicate);

    // $nin requires an array.
    let err = where_query_err(json!({ "country": { "$nin": "HR" } }));
    assert_eq!(err.code, ErrorCode::InvalidPropertyPredicate);

    // $not requires an object holding exactly one inner operator.
    let err = where_query_err(json!({ "active": { "$not": true } }));
    assert_eq!(err.code, ErrorCode::InvalidPropertyPredicate);

    let err = where_query_err(json!({ "active": { "$not": {} } }));
    assert_eq!(err.code, ErrorCode::InvalidPropertyPredicate);
}

fn where_query_err(where_clause: serde_json::Value) -> SpatialError {
    Query::from_json(&json!({
        "spatial": { "predicate": "intersects" },
        "where": where_clause
    }))
    .unwrap_err()
}
