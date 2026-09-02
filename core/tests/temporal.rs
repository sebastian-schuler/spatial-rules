//! Integration tests for temporal conditions: query `at` + whole-clause
//! `$activeAt` over scalar rule window properties (ADR-0017, P2 ticket 02).
//!
//! Expected values are hand-computed window-membership literals: `daysOfWeek`
//! is an Int bitmask (Mon=1 … Sun=64), hours are Int `0..=23`, admission is
//! start-inclusive / end-exclusive with midnight wrap.

use spatial_rules_core::{
    candidates_from_geojson, ErrorCode, PropertyValue, Query, ResolutionOutcome, Rule, Ruleset,
    SpatialPredicate, TemporalInstant, WhereExpr,
};

mod common;
use common::{candidate, rule_with_props, square};

// Mon-Fri = 1+2+4+8+16 = 31; Mon-Sun = 127.
const WEEKDAYS: i64 = 31;
const EVERY_DAY: i64 = 127;

/// A rule with a temporal window property set (day bitmask + hour window).
fn window_rule(id: &str, days: i64, start: i64, end: i64) -> Rule {
    rule_with_props(
        id,
        square(0.0, 0.0, 10.0, 10.0),
        &[
            ("daysOfWeek", PropertyValue::Int(days)),
            ("startHour", PropertyValue::Int(start)),
            ("endHour", PropertyValue::Int(end)),
        ],
    )
}

/// A query admitting rules whose `daysOfWeek`/`startHour`/`endHour` window
/// contains the reference `at`.
fn temporal_query(at: &str) -> Query {
    Query::from_json(&serde_json::json!({
        "spatial": { "predicate": "intersects" },
        "at": at,
        "where": { "$activeAt": { "daysOfWeek": "daysOfWeek", "startHour": "startHour", "endHour": "endHour" } }
    }))
    .unwrap()
}

fn inside() -> spatial_rules_core::Candidate {
    candidate("c", square(2.0, 2.0, 4.0, 4.0))
}

#[test]
fn active_window_admits_inside_and_rejects_outside() {
    // Mon-Fri 09:00-17:00. Reference dates: 2026-08-24 is a Monday, 2026-08-29
    // is a Saturday.
    let ruleset = Ruleset::build(vec![window_rule("parking", WEEKDAYS, 9, 17)]).unwrap();
    let candidate = inside();

    assert_eq!(
        ruleset.query_mask(std::slice::from_ref(&candidate), &temporal_query("2026-08-24T10:00")),
        vec![1],
        "Monday 10:00 is inside Mon-Fri 09-17"
    );
    assert_eq!(
        ruleset.query_mask(std::slice::from_ref(&candidate), &temporal_query("2026-08-24T17:00")),
        vec![0],
        "the end hour is exclusive: 17:00 is outside 09-17"
    );
    assert_eq!(
        ruleset.query_mask(std::slice::from_ref(&candidate), &temporal_query("2026-08-24T08:59")),
        vec![0],
        "08:59 is before the start"
    );
    assert_eq!(
        ruleset.query_mask(std::slice::from_ref(&candidate), &temporal_query("2026-08-29T10:00")),
        vec![0],
        "Saturday is not in Mon-Fri"
    );
}

#[test]
fn midnight_crossing_window_wraps() {
    // Active 22:00-06:00 (wraps midnight).
    let ruleset = Ruleset::build(vec![window_rule("night", EVERY_DAY, 22, 6)]).unwrap();
    let candidate = inside();

    assert_eq!(
        ruleset.query_mask(std::slice::from_ref(&candidate), &temporal_query("2026-08-24T23:00")),
        vec![1],
        "23:00 is inside 22-06"
    );
    assert_eq!(
        ruleset.query_mask(std::slice::from_ref(&candidate), &temporal_query("2026-08-25T05:00")),
        vec![1],
        "05:00 next day is inside 22-06"
    );
    assert_eq!(
        ruleset.query_mask(std::slice::from_ref(&candidate), &temporal_query("2026-08-25T12:00")),
        vec![0],
        "12:00 is outside 22-06"
    );
}

#[test]
fn empty_window_never_admits() {
    // startHour == endHour is an empty window under start-inclusive/end-exclusive.
    let ruleset = Ruleset::build(vec![window_rule("empty", EVERY_DAY, 9, 9)]).unwrap();
    assert_eq!(
        ruleset.query_mask(&[inside()], &temporal_query("2026-08-24T09:00")),
        vec![0]
    );
}

#[test]
fn days_of_week_zero_never_admits() {
    let ruleset = Ruleset::build(vec![window_rule("off", 0, 0, 24)]).unwrap();
    assert_eq!(
        ruleset.query_mask(&[inside()], &temporal_query("2026-08-24T10:00")),
        vec![0]
    );
}

#[test]
fn missing_or_non_integer_window_field_is_a_non_match() {
    // A rule without the window properties, and one with a string daysOfWeek,
    // are never admitted (missing property / type mismatch = non-match).
    let ruleset = Ruleset::build(vec![
        rule_with_props("no-window", square(0.0, 0.0, 10.0, 10.0), &[]),
        rule_with_props(
            "str-days",
            square(0.0, 0.0, 10.0, 10.0),
            &[
                ("daysOfWeek", PropertyValue::Str("31".into())),
                ("startHour", PropertyValue::Int(9)),
                ("endHour", PropertyValue::Int(17)),
            ],
        ),
    ])
    .unwrap();
    let outcomes = ruleset.query(&[inside()], &temporal_query("2026-08-24T10:00"));
    assert_eq!(outcomes, vec![spatial_rules_core::CandidateOutcome::NotMatched]);
}

#[test]
fn active_at_composes_under_and_or_nor() {
    let ruleset = Ruleset::build(vec![
        window_rule("parking", WEEKDAYS, 9, 17),
        {
            // Active at every hour of every day, plus a plain boolean property.
            let mut rule = window_rule("always", EVERY_DAY, 0, 24);
            rule.properties
                .insert("active".to_string(), PropertyValue::Bool(true));
            rule
        },
    ])
    .unwrap();
    let candidate = inside();

    // $and: temporal + a plain predicate.
    let and = Query::from_json(&serde_json::json!({
        "spatial": { "predicate": "intersects" },
        "at": "2026-08-24T10:00",
        "where": { "$and": [ { "$activeAt": { "daysOfWeek": "daysOfWeek", "startHour": "startHour", "endHour": "endHour" } }, { "active": true } ] }
    }))
    .unwrap();
    // Only "always" satisfies both (parking has no active=true property).
    let outcomes = ruleset.query(std::slice::from_ref(&candidate), &and);
    assert!(matches!(
        &outcomes[0],
        spatial_rules_core::CandidateOutcome::Matched { rule_ids, .. }
            if rule_ids == &vec![ruleset.rule_id("always").unwrap()]
    ));

    // $or: temporal OR a plain predicate — the parking rule is admitted on a
    // Monday even though "always" matches anyway.
    let or = Query::from_json(&serde_json::json!({
        "spatial": { "predicate": "intersects" },
        "at": "2026-08-24T10:00",
        "where": { "$or": [ { "$activeAt": { "daysOfWeek": "daysOfWeek", "startHour": "startHour", "endHour": "endHour" } }, { "active": false } ] }
    }))
    .unwrap();
    let outcomes = ruleset.query(std::slice::from_ref(&candidate), &or);
    assert!(matches!(
        &outcomes[0],
        spatial_rules_core::CandidateOutcome::Matched { rule_ids, .. }
            if rule_ids == &vec![
                ruleset.rule_id("parking").unwrap(),
                ruleset.rule_id("always").unwrap(),
            ]
    ));

    // $nor: temporal NOT — on a Saturday the parking rule is admitted by $nor
    // (its window is inactive), while "always" is temporally active so $nor
    // rejects it.
    let nor = Query::from_json(&serde_json::json!({
        "spatial": { "predicate": "intersects" },
        "at": "2026-08-29T10:00",
        "where": { "$nor": [ { "$activeAt": { "daysOfWeek": "daysOfWeek", "startHour": "startHour", "endHour": "endHour" } } ] }
    }))
    .unwrap();
    let outcomes = ruleset.query(std::slice::from_ref(&candidate), &nor);
    assert!(matches!(
        &outcomes[0],
        spatial_rules_core::CandidateOutcome::Matched { rule_ids, .. }
            if rule_ids == &vec![ruleset.rule_id("parking").unwrap()]
    ));
}

#[test]
fn at_is_required_when_active_at_present() {
    let err = Query::from_json(&serde_json::json!({
        "spatial": { "predicate": "intersects" },
        "where": { "$activeAt": { "daysOfWeek": "daysOfWeek", "startHour": "startHour", "endHour": "endHour" } }
    }))
    .unwrap_err();
    assert_eq!(err.code, ErrorCode::InvalidQuery);
    assert!(err.message.contains("'at' is required"));
}

#[test]
fn malformed_at_is_rejected_even_without_active_at() {
    let err = Query::from_json(&serde_json::json!({
        "spatial": { "predicate": "intersects" },
        "at": "not-a-date"
    }))
    .unwrap_err();
    assert_eq!(err.code, ErrorCode::InvalidQuery);
    assert!(err.message.contains("invalid 'at'"));
}

#[test]
fn unused_at_is_allowed_and_validated() {
    // A present-but-unused `at` parses fine (no $activeAt in the query).
    let query = Query::from_json(&serde_json::json!({
        "spatial": { "predicate": "intersects" },
        "at": "2026-08-24T10:00"
    }))
    .unwrap();
    assert!(query.at.is_some());
    // A non-string `at` is rejected.
    let err = Query::from_json(&serde_json::json!({
        "spatial": { "predicate": "intersects" },
        "at": 5
    }))
    .unwrap_err();
    assert_eq!(err.code, ErrorCode::InvalidQuery);
}

#[test]
fn malformed_active_at_clause_is_rejected() {
    let err = Query::from_json(&serde_json::json!({
        "spatial": { "predicate": "intersects" },
        "at": "2026-08-24T10:00",
        "where": { "$activeAt": { "daysOfWeek": "daysOfWeek" } }
    }))
    .unwrap_err();
    assert_eq!(err.code, ErrorCode::InvalidPropertyPredicate);

    let err = Query::from_json(&serde_json::json!({
        "spatial": { "predicate": "intersects" },
        "at": "2026-08-24T10:00",
        "where": { "$activeAt": "daysOfWeek" }
    }))
    .unwrap_err();
    assert_eq!(err.code, ErrorCode::InvalidPropertyPredicate);
}

#[test]
fn temporal_admission_feeds_resolution() {
    // Resolution admits only temporally-active rules, and the winner is the
    // highest-priority one among them.
    let ruleset = Ruleset::build(vec![
        {
            let mut rule = window_rule("weekday", WEEKDAYS, 9, 17);
            rule.priority = 10;
            rule
        },
        {
            let mut rule = window_rule("weekend", 32 + 64, 9, 17); // Sat+Sun
            rule.priority = 5;
            rule
        },
    ])
    .unwrap();
    let candidates = candidates_from_geojson(r#"{
        "type": "FeatureCollection",
        "features": [
            { "type": "Feature", "id": "c", "properties": {}, "geometry": { "type": "Polygon", "coordinates": [[[2, 2], [2, 4], [4, 4], [4, 2], [2, 2]]] } }
        ]
    }"#)
    .unwrap();

    // Monday: only the weekday rule is active.
    let outcomes = ruleset.resolve(&candidates, &temporal_query("2026-08-24T10:00"));
    let ResolutionOutcome::Resolved { winner, applicable, .. } = &outcomes[0] else {
        panic!("expected a resolved outcome");
    };
    assert_eq!(*winner, ruleset.rule_id("weekday").unwrap());
    assert_eq!(applicable.len(), 1);

    // Saturday: only the weekend rule is active.
    let outcomes = ruleset.resolve(&candidates, &temporal_query("2026-08-29T10:00"));
    let ResolutionOutcome::Resolved { winner, .. } = &outcomes[0] else {
        panic!("expected a resolved outcome");
    };
    assert_eq!(*winner, ruleset.rule_id("weekend").unwrap());
}

#[test]
fn malformed_programmatic_temporal_instant_is_a_non_match_not_a_panic() {
    // `new` guards the day/hour ranges, but a caller that still cannot
    // construct an invalid instant is the point: the parse and constructor
    // both enforce `1..=7` and `0..=23`, so a malformed instant is impossible
    // to build and the window evaluation never sees an underflowing shift.
    let ruleset = Ruleset::build(vec![window_rule("parking", WEEKDAYS, 9, 17)]).unwrap();
    // A boundary value (day 0) is rejected at construction, so the no-panic
    // guarantee is structural rather than a runtime guard.
    assert_eq!(TemporalInstant::new(0, 10), None);
    let bad = Query::new(SpatialPredicate::Intersects)
        .with_at(TemporalInstant::new(7, 10).unwrap())
        .with_where(
            WhereExpr::parse(&serde_json::json!({
                "$activeAt": { "daysOfWeek": "daysOfWeek", "startHour": "startHour", "endHour": "endHour" }
            }))
            .unwrap(),
        );
    assert_eq!(ruleset.query_mask(&[inside()], &bad), vec![0]);
}

#[test]
fn plain_queries_are_unaffected_by_the_new_fields() {
    // `query()` with no `at`/`$activeAt` behaves exactly as before.
    let ruleset = Ruleset::build(vec![window_rule("parking", WEEKDAYS, 9, 17)]).unwrap();
    assert_eq!(
        ruleset.query_mask(&[inside()], &Query::new(SpatialPredicate::Intersects)),
        vec![1]
    );
    // `at` alone (no temporal predicate) never changes admission.
    let with_at = Query::from_json(&serde_json::json!({
        "spatial": { "predicate": "intersects" },
        "at": "2026-08-24T10:00"
    }))
    .unwrap();
    assert_eq!(ruleset.query_mask(&[inside()], &with_at), vec![1]);
}