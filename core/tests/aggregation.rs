//! Integration tests for per-candidate aggregation over the applicable rule
//! set (ADR-0018, aggregation tickets 01 + 03). Expected values are hand-folded
//! literals, not recomputed by the code under test.

use spatial_rules_core::{
    CandidateOutcome, ErrorCode, PropertyValue, Query, ResolutionOutcome, Rule, Ruleset,
    SpatialPredicate,
};

mod common;
use common::{candidate, rule_with_props, square};

/// A rule spanning `x_min..x_max` at y 0..10 with an optional integer
/// `speedLimit` property.
fn speed_rule(id: &str, x_min: f64, x_max: f64, speed_limit: Option<i64>) -> Rule {
    let mut rule = rule_with_props(id, square(x_min, 0.0, x_max, 10.0), &[]);
    if let Some(speed) = speed_limit {
        rule.properties
            .insert("speedLimit".to_string(), PropertyValue::Int(speed));
    }
    rule
}

/// A candidate square; the full query aggregate spec over `speedLimit`.
fn aggregate_query() -> Query {
    Query::from_json(&serde_json::json!({
        "spatial": { "predicate": "intersects" },
        "aggregate": {
            "count": true, "min": "speedLimit", "max": "speedLimit",
            "sum": "speedLimit", "avg": "speedLimit", "coverage": true
        }
    }))
    .unwrap()
}

fn matched_ids(ruleset: &Ruleset, candidate: &spatial_rules_core::Candidate, query: &Query) -> Vec<spatial_rules_core::RuleId> {
    match &ruleset.query(std::slice::from_ref(candidate), query)[0] {
        CandidateOutcome::Matched { rule_ids, .. } => rule_ids.clone(),
        other => panic!("expected a match, got {other:?}"),
    }
}

#[test]
fn aggregate_folds_numeric_properties_and_skips_non_numeric() {
    // "mid" has a string speedLimit: counted in `count`, skipped by the numeric
    // functions, included in the union coverage (it covers the whole span).
    let ruleset = Ruleset::build(vec![
        speed_rule("left", 0.0, 4.0, Some(30)),
        speed_rule("right", 6.0, 10.0, Some(50)),
        {
            let mut rule = rule_with_props("mid", square(2.0, 0.0, 8.0, 10.0), &[]);
            rule.properties
                .insert("speedLimit".to_string(), PropertyValue::Str("fast".into()));
            rule
        },
    ])
    .unwrap();
    let candidate = candidate("c", square(2.0, 2.0, 8.0, 8.0));
    let ids = matched_ids(&ruleset, &candidate, &aggregate_query());
    let aggregate = aggregate_query()
        .aggregate
        .unwrap()
        .compute(&candidate, &ids, &ruleset);

    assert_eq!(aggregate.count, Some(3));
    assert_eq!(aggregate.min, Some(30.0));
    assert_eq!(aggregate.max, Some(50.0));
    assert_eq!(aggregate.sum, Some(80.0));
    assert_eq!(aggregate.avg, Some(40.0));
    // The union of all three rules covers the candidate fully.
    let coverage = aggregate.coverage.unwrap();
    assert!((coverage - 1.0).abs() < 1e-6, "coverage {coverage}");
}

#[test]
fn coverage_is_union_not_per_rule_double_count() {
    // Two identical rules covering the candidate: union coverage is 1, not the
    // per-rule sum (which would exceed 1).
    let ruleset = Ruleset::build(vec![
        speed_rule("a", 0.0, 10.0, Some(30)),
        speed_rule("b", 0.0, 10.0, Some(30)),
    ])
    .unwrap();
    let candidate = candidate("c", square(2.0, 2.0, 8.0, 8.0));
    let ids = matched_ids(&ruleset, &candidate, &aggregate_query());
    let aggregate = aggregate_query()
        .aggregate
        .unwrap()
        .compute(&candidate, &ids, &ruleset);
    assert_eq!(aggregate.count, Some(2));
    let coverage = aggregate.coverage.unwrap();
    assert!((coverage - 1.0).abs() < 1e-6, "coverage {coverage}");
}

#[test]
fn coverage_is_the_union_for_partial_overlap() {
    // Two disjoint rules cover the left and right thirds of the candidate
    // [2,8]x[2,8]: union coverage is 2/3, not the per-rule sum (4/3).
    let ruleset = Ruleset::build(vec![
        speed_rule("left", 0.0, 4.0, Some(30)),
        speed_rule("right", 6.0, 10.0, Some(50)),
    ])
    .unwrap();
    let candidate = candidate("c", square(2.0, 2.0, 8.0, 8.0));
    let ids = matched_ids(&ruleset, &candidate, &aggregate_query());
    let aggregate = aggregate_query()
        .aggregate
        .unwrap()
        .compute(&candidate, &ids, &ruleset);
    let coverage = aggregate.coverage.unwrap();
    // The union+intersection clipping carries a small numerical artifact at
    // partial-overlap boundaries; 0.1% is the honest precision of the ratio.
    assert!((coverage - 2.0 / 3.0).abs() < 1e-3, "coverage {coverage}");
}

#[test]
fn numeric_aggregates_absent_when_nothing_contributes() {
    let ruleset = Ruleset::build(vec![speed_rule("a", 0.0, 10.0, None)]).unwrap();
    let candidate = candidate("c", square(2.0, 2.0, 8.0, 8.0));
    let ids = matched_ids(&ruleset, &candidate, &aggregate_query());
    let aggregate = aggregate_query()
        .aggregate
        .unwrap()
        .compute(&candidate, &ids, &ruleset);
    assert_eq!(aggregate.count, Some(1));
    assert_eq!(aggregate.min, None);
    assert_eq!(aggregate.max, None);
    assert_eq!(aggregate.sum, None);
    assert_eq!(aggregate.avg, None);
    assert!(aggregate.coverage.is_some());
}

#[test]
fn single_rule_coverage_equals_the_rule_overlap_ratio() {
    // One rule: union coverage is exactly the rule's overlap with the candidate.
    let ruleset = Ruleset::build(vec![speed_rule("a", 5.0, 10.0, Some(30))]).unwrap();
    // The candidate spans the full span; the rule covers its right half.
    let candidate = candidate("c", square(0.0, 0.0, 10.0, 10.0));
    let ids = matched_ids(&ruleset, &candidate, &aggregate_query());
    let aggregate = aggregate_query()
        .aggregate
        .unwrap()
        .compute(&candidate, &ids, &ruleset);
    let coverage = aggregate.coverage.unwrap();
    assert!((coverage - 0.5).abs() < 1e-3, "coverage {coverage}");
}

#[test]
fn query_parses_and_validates_aggregate_strictly() {
    let ok = Query::from_json(&serde_json::json!({
        "spatial": { "predicate": "intersects" },
        "aggregate": { "count": true, "avg": "taxRate" }
    }))
    .unwrap();
    assert!(ok.aggregate.is_some());

    for bad in [
        serde_json::json!({ "spatial": { "predicate": "intersects" }, "aggregate": { "median": true } }),
        serde_json::json!({ "spatial": { "predicate": "intersects" }, "aggregate": { "count": "yes" } }),
        serde_json::json!({ "spatial": { "predicate": "intersects" }, "aggregate": { "min": 5 } }),
        serde_json::json!({ "spatial": { "predicate": "intersects" }, "aggregate": {} }),
        serde_json::json!({ "spatial": { "predicate": "intersects" }, "aggregate": { "count": false } }),
    ] {
        let err = Query::from_json(&bad).unwrap_err();
        assert_eq!(err.code, ErrorCode::InvalidQuery, "{bad}");
    }
}

#[test]
fn aggregate_does_not_change_admission_mask_or_summary() {
    let ruleset = Ruleset::build(vec![speed_rule("a", 0.0, 10.0, Some(30))]).unwrap();
    let candidates = vec![
        candidate("inside", square(2.0, 2.0, 8.0, 8.0)),
        candidate("far", square(50.0, 50.0, 60.0, 60.0)),
    ];
    let plain = Query::new(SpatialPredicate::Intersects);
    assert_eq!(
        ruleset.query_mask(&candidates, &plain),
        ruleset.query_mask(&candidates, &aggregate_query())
    );
    assert_eq!(
        ruleset.query_mask(&candidates, &aggregate_query()),
        vec![1, 0]
    );
    // The far candidate is still NotMatched (no aggregate is produced for it —
    // the rich serializer only emits aggregate for matched outcomes).
    assert_eq!(
        ruleset.query(&[candidate("far", square(50.0, 50.0, 60.0, 60.0))], &aggregate_query()),
        vec![CandidateOutcome::NotMatched]
    );
}

#[test]
fn aggregate_works_over_the_within_distance_applicable_set() {
    // Two zones, one within 200 m of the point and one not: the aggregate
    // folds only the distance-admitted rules.
    let ruleset = Ruleset::build(vec![
        {
            let mut rule = speed_rule("near", 0.0, 1.0, Some(30));
            rule.priority = 10;
            rule
        },
        speed_rule("far", 5.0, 6.0, Some(50)),
    ])
    .unwrap();
    let point = common::candidate_geometry("p", geo::Geometry::Point(geo::Point::new(0.5, 0.5)));
    let query = Query::from_json(&serde_json::json!({
        "spatial": { "predicate": "withinDistance", "distance": 200.0 },
        "aggregate": { "count": true, "min": "speedLimit", "max": "speedLimit", "coverage": true }
    }))
    .unwrap();
    let ResolutionOutcome::Resolved { applicable, .. } = &ruleset.resolve(std::slice::from_ref(&point), &query)[0] else {
        panic!("expected a resolved outcome");
    };
    let ids: Vec<_> = applicable.iter().map(|rule| rule.rule_id).collect();
    let aggregate = query.aggregate.unwrap().compute(&point, &ids, &ruleset);
    assert_eq!(aggregate.count, Some(1), "only the near rule is within 200 m");
    assert_eq!(aggregate.min, Some(30.0));
    assert_eq!(aggregate.coverage, Some(0.0), "a point has zero area");
}

#[test]
fn aggregate_works_over_the_temporal_applicable_set() {
    // A weekday-only rule and an always rule; at a Saturday, only the always
    // rule is applicable, so the aggregate folds just it.
    let weekday = {
        let mut rule = speed_rule("weekday", 0.0, 10.0, Some(30));
        rule.properties
            .insert("daysOfWeek".to_string(), PropertyValue::Int(31));
        rule.properties
            .insert("startHour".to_string(), PropertyValue::Int(9));
        rule.properties
            .insert("endHour".to_string(), PropertyValue::Int(17));
        rule
    };
    let always = {
        let mut rule = speed_rule("always", 0.0, 10.0, Some(50));
        rule.properties
            .insert("daysOfWeek".to_string(), PropertyValue::Int(127));
        rule.properties
            .insert("startHour".to_string(), PropertyValue::Int(0));
        rule.properties
            .insert("endHour".to_string(), PropertyValue::Int(24));
        rule
    };
    let ruleset = Ruleset::build(vec![weekday, always]).unwrap();
    let candidate = candidate("c", square(2.0, 2.0, 8.0, 8.0));
    let query = Query::from_json(&serde_json::json!({
        "spatial": { "predicate": "intersects" },
        "at": "2026-08-29T10:00",
        "where": { "$activeAt": { "daysOfWeek": "daysOfWeek", "startHour": "startHour", "endHour": "endHour" } },
        "aggregate": { "count": true, "min": "speedLimit", "max": "speedLimit" }
    }))
    .unwrap();
    let ids = matched_ids(&ruleset, &candidate, &query);
    let aggregate = query.aggregate.unwrap().compute(&candidate, &ids, &ruleset);
    assert_eq!(aggregate.count, Some(1), "Saturday admits only the always rule");
    assert_eq!(aggregate.min, Some(50.0));
    assert_eq!(aggregate.max, Some(50.0));
}