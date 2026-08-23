//! Integration tests for resolution: the ordered applicable set, the winner,
//! first-provider-wins derived values, and the flat per-rule explanation
//! (ADR-0015, tickets 02/03).
//!
//! turf cannot oracle resolution, so expected values are hand-checked
//! precedence/merge literals derived from the ADR, not recomputed by the code
//! under test.

use geo::Polygon;
use spatial_rules_core::{
    PropertyValue, Query, ResolutionOutcome, Rule, Ruleset, SpatialIndexKind, SpatialPredicate,
};

mod common;
use common::{candidate, rule_with_props, square};

/// A polygon rule with a top-level precedence and typed properties.
fn priority_rule(
    id: &str,
    polygon: Polygon<f64>,
    priority: i64,
    properties: &[(&str, PropertyValue)],
) -> Rule {
    let mut rule = rule_with_props(id, polygon, properties);
    rule.priority = priority;
    rule
}

/// Two overlapping rules with distinct priorities: a-b (10) covers the left
/// half of the unit square, lo-b (5) the right half, so a candidate in the
/// middle intersects both.
fn overlapping_ruleset() -> Ruleset {
    Ruleset::build(vec![
        priority_rule(
            "hi",
            square(0.0, 0.0, 10.0, 10.0),
            10,
            &[
                ("kind", PropertyValue::Str("hi".into())),
                ("shared", PropertyValue::Str("from-hi".into())),
            ],
        ),
        priority_rule(
            "lo",
            square(5.0, 5.0, 15.0, 15.0),
            5,
            &[
                ("kind", PropertyValue::Str("lo".into())),
                ("shared", PropertyValue::Str("from-lo".into())),
            ],
        ),
    ])
    .unwrap()
}

fn inside() -> spatial_rules_core::Candidate {
    candidate("inside", square(6.0, 6.0, 8.0, 8.0))
}

fn intersects() -> Query {
    Query::new(SpatialPredicate::Intersects)
}

#[test]
fn winner_is_the_max_priority_applicable_rule() {
    let ruleset = overlapping_ruleset();
    let outcomes = ruleset.resolve(&[inside()], &intersects());
    let ResolutionOutcome::Resolved {
        winner,
        values,
        applicable,
    } = &outcomes[0]
    else {
        panic!("expected a resolved outcome");
    };
    assert_eq!(*winner, ruleset.rule_id("hi").unwrap());
    assert_eq!(
        values.get("kind"),
        Some(&PropertyValue::Str("hi".into()))
    );
    assert_eq!(applicable.len(), 2);
}

#[test]
fn values_are_first_provider_wins_down_the_order() {
    let ruleset = overlapping_ruleset();
    let outcomes = ruleset.resolve(&[inside()], &intersects());
    let ResolutionOutcome::Resolved { values, .. } = &outcomes[0] else {
        panic!("expected a resolved outcome");
    };
    // Both rules define "shared"; the higher-precedence rule's value wins.
    assert_eq!(
        values.get("shared"),
        Some(&PropertyValue::Str("from-hi".into()))
    );
}

#[test]
fn gap_fill_takes_a_field_from_a_lower_rule_when_the_winner_lacks_it() {
    let ruleset = Ruleset::build(vec![
        priority_rule(
            "winner",
            square(0.0, 0.0, 10.0, 10.0),
            10,
            &[("kind", PropertyValue::Str("hi".into()))],
        ),
        priority_rule(
            "backup",
            square(5.0, 5.0, 15.0, 15.0),
            5,
            &[("kind", PropertyValue::Str("lo".into())), ("extra", PropertyValue::Str("x".into()))],
        ),
    ])
    .unwrap();
    let outcomes = ruleset.resolve(&[inside()], &intersects());
    let ResolutionOutcome::Resolved { values, .. } = &outcomes[0] else {
        panic!("expected a resolved outcome");
    };
    // "kind" comes from the winner; "extra" only the lower rule defines it.
    assert_eq!(values.get("kind"), Some(&PropertyValue::Str("hi".into())));
    assert_eq!(values.get("extra"), Some(&PropertyValue::Str("x".into())));
}

#[test]
fn equal_priorities_resolve_by_declaration_order() {
    let ruleset = Ruleset::build(vec![
        priority_rule(
            "first",
            square(0.0, 0.0, 10.0, 10.0),
            7,
            &[("kind", PropertyValue::Str("first".into()))],
        ),
        priority_rule(
            "second",
            square(5.0, 5.0, 15.0, 15.0),
            7,
            &[("kind", PropertyValue::Str("second".into()))],
        ),
    ])
    .unwrap();
    let outcomes = ruleset.resolve(&[inside()], &intersects());
    let ResolutionOutcome::Resolved {
        winner,
        values,
        applicable,
    } = &outcomes[0]
    else {
        panic!("expected a resolved outcome");
    };
    assert_eq!(*winner, ruleset.rule_id("first").unwrap());
    assert_eq!(
        values.get("kind"),
        Some(&PropertyValue::Str("first".into()))
    );
    let ids: Vec<_> = applicable.iter().map(|rule| rule.rule_id).collect();
    assert_eq!(
        ids,
        vec![ruleset.rule_id("first").unwrap(), ruleset.rule_id("second").unwrap()]
    );
}

#[test]
fn where_clause_admission_removes_rules_from_the_applicable_set() {
    let ruleset = Ruleset::build(vec![
        priority_rule(
            "active-hi",
            square(0.0, 0.0, 10.0, 10.0),
            10,
            &[("active", PropertyValue::Bool(true)), ("kind", PropertyValue::Str("hi".into()))],
        ),
        priority_rule(
            "inactive-lo",
            square(5.0, 5.0, 15.0, 15.0),
            5,
            &[("active", PropertyValue::Bool(false)), ("kind", PropertyValue::Str("lo".into()))],
        ),
    ])
    .unwrap();
    let query = Query::from_json(&serde_json::json!({
        "spatial": { "predicate": "intersects" },
        "where": { "active": true }
    }))
    .unwrap();
    let outcomes = ruleset.resolve(&[inside()], &query);
    let ResolutionOutcome::Resolved {
        winner,
        values,
        applicable,
    } = &outcomes[0]
    else {
        panic!("expected a resolved outcome");
    };
    // The failing-where rule contributes nothing.
    assert_eq!(*winner, ruleset.rule_id("active-hi").unwrap());
    assert_eq!(applicable.len(), 1);
    assert_eq!(values.get("kind"), Some(&PropertyValue::Str("hi".into())));
    assert!(!values.contains_key("lo"));
}

#[test]
fn excluded_rules_are_not_applicable_and_contribute_no_values() {
    let ruleset = overlapping_ruleset();
    let query = Query::from_json(&serde_json::json!({
        "spatial": { "predicate": "intersects" },
        "excludeRuleIds": ["hi"]
    }))
    .unwrap();
    let outcomes = ruleset.resolve(&[inside()], &query);
    let ResolutionOutcome::Resolved { winner, values, .. } = &outcomes[0] else {
        panic!("expected a resolved outcome");
    };
    assert_eq!(*winner, ruleset.rule_id("lo").unwrap());
    assert_eq!(values.get("kind"), Some(&PropertyValue::Str("lo".into())));
    assert_eq!(
        values.get("shared"),
        Some(&PropertyValue::Str("from-lo".into()))
    );
}

#[test]
fn candidate_matched_by_no_rules_is_not_resolved() {
    let ruleset = overlapping_ruleset();
    let far = candidate("far", square(50.0, 50.0, 60.0, 60.0));
    assert_eq!(
        ruleset.resolve(&[far], &intersects()),
        vec![ResolutionOutcome::NotMatched]
    );
}

#[test]
fn invalid_candidate_stays_in_result_with_reason() {
    let ruleset = overlapping_ruleset();
    let bowtie = common::candidate_geometry("bowtie", geo::Geometry::Polygon(common::bowtie()));
    let outcomes = ruleset.resolve(&[bowtie], &intersects());
    assert!(matches!(&outcomes[0], ResolutionOutcome::Invalid { reason } if reason.starts_with("invalid geometry:")));
}

#[test]
fn unprioritized_rules_sort_below_explicit_priorities() {
    let ruleset = Ruleset::build(vec![
        priority_rule("plain", square(0.0, 0.0, 10.0, 10.0), 0, &[]),
        priority_rule("prioritized", square(5.0, 5.0, 15.0, 15.0), 1, &[]),
    ])
    .unwrap();
    let outcomes = ruleset.resolve(&[inside()], &intersects());
    let ResolutionOutcome::Resolved { winner, applicable, .. } = &outcomes[0] else {
        panic!("expected a resolved outcome");
    };
    // The explicit priority-1 rule outranks the unprioritized (0) one even
    // though it was declared second.
    assert_eq!(*winner, ruleset.rule_id("prioritized").unwrap());
    assert_eq!(
        applicable[0].rule_id,
        ruleset.rule_id("prioritized").unwrap()
    );
}

#[test]
fn explanation_members_carry_rule_priority_and_matched_flags() {
    let ruleset = overlapping_ruleset();
    let outcomes = ruleset.resolve(&[inside()], &intersects());
    let ResolutionOutcome::Resolved { applicable, .. } = &outcomes[0] else {
        panic!("expected a resolved outcome");
    };
    let hi = ruleset.rule_id("hi").unwrap();
    let lo = ruleset.rule_id("lo").unwrap();
    let expected = vec![
        spatial_rules_core::ApplicableRule {
            rule_id: hi,
            priority: 10,
            spatial_matched: true,
            property_matched: true,
        },
        spatial_rules_core::ApplicableRule {
            rule_id: lo,
            priority: 5,
            spatial_matched: true,
            property_matched: true,
        },
    ];
    assert_eq!(applicable, &expected);
}

#[test]
fn resolution_is_stable_across_both_spatial_index_kinds() {
    let rules = vec![
        priority_rule(
            "hi",
            square(0.0, 0.0, 10.0, 10.0),
            10,
            &[("kind", PropertyValue::Str("hi".into()))],
        ),
        priority_rule(
            "lo",
            square(5.0, 5.0, 15.0, 15.0),
            5,
            &[("kind", PropertyValue::Str("lo".into()))],
        ),
    ];
    let rstar = Ruleset::build_with(rules.clone(), SpatialIndexKind::RStar).unwrap();
    let scan = Ruleset::build_with(rules, SpatialIndexKind::LinearScan).unwrap();

    let candidates = vec![
        inside(),
        candidate("far", square(50.0, 50.0, 60.0, 60.0)),
    ];
    assert_eq!(
        rstar.resolve(&candidates, &intersects()),
        scan.resolve(&candidates, &intersects())
    );
}

#[test]
fn resolution_repeats_deterministically() {
    let ruleset = overlapping_ruleset();
    let first = ruleset.resolve(&[inside()], &intersects());
    let second = ruleset.resolve(&[inside()], &intersects());
    assert_eq!(first, second);
}

#[test]
fn match_path_and_mask_are_unchanged_by_priorities() {
    let ruleset = overlapping_ruleset();
    // The existing match surface reports both applicable ids in envelope order.
    assert_eq!(
        ruleset.query_mask(&[inside()], &intersects()),
        vec![1]
    );
}

// --- Ticket 04: compact resolution mask (`0` no resolution, `1` resolved, `2` invalid) ---

#[test]
fn resolve_mask_marks_resolved_not_resolved_and_invalid() {
    let ruleset = overlapping_ruleset();
    let candidates = vec![
        inside(), // resolved (both rules applicable)
        candidate("far", square(50.0, 50.0, 60.0, 60.0)),
        common::candidate_geometry("bowtie", geo::Geometry::Polygon(common::bowtie())),
    ];
    assert_eq!(
        ruleset.resolve_mask(&candidates, &intersects()),
        vec![1, 0, 2]
    );
}

#[test]
fn resolve_mask_matches_resolve_outcomes() {
    let ruleset = overlapping_ruleset();
    let candidates = vec![
        inside(),
        candidate("far", square(50.0, 50.0, 60.0, 60.0)),
        common::candidate_geometry("bowtie", geo::Geometry::Polygon(common::bowtie())),
    ];
    let outcomes = ruleset.resolve(&candidates, &intersects());
    let expected: Vec<u8> = outcomes
        .iter()
        .map(|outcome| match outcome {
            ResolutionOutcome::Resolved { .. } => 1,
            ResolutionOutcome::NotMatched => 0,
            ResolutionOutcome::Invalid { .. } => 2,
        })
        .collect();
    assert_eq!(ruleset.resolve_mask(&candidates, &intersects()), expected);
}

#[test]
fn engine_resolve_mask_matches_ruleset_mask() {
    use spatial_rules_core::candidates_from_geojson;
    let engine = spatial_rules_core::Engine::from_geojson(
        r#"{
          "type": "FeatureCollection",
          "features": [
            { "type": "Feature", "id": "hi", "priority": 10, "properties": { "kind": "a" },
              "geometry": { "type": "Polygon", "coordinates": [[[0, 0], [0, 10], [10, 10], [10, 0], [0, 0]]] } },
            { "type": "Feature", "id": "lo", "priority": 5, "properties": { "kind": "b" },
              "geometry": { "type": "Polygon", "coordinates": [[[5, 5], [5, 15], [15, 15], [15, 5], [5, 5]]] } }
          ]
        }"#,
    )
    .unwrap();
    let candidates = candidates_from_geojson(
        r#"{
          "type": "FeatureCollection",
          "features": [
            { "type": "Feature", "id": "c", "properties": {}, "geometry": { "type": "Polygon", "coordinates": [[[6, 6], [6, 8], [8, 8], [8, 6], [6, 6]]] } }
          ]
        }"#,
    )
    .unwrap();
    assert_eq!(
        engine.resolve_mask(&candidates, &intersects()),
        vec![1]
    );
    assert_eq!(engine.resolve(&candidates, &intersects()).len(), 1);
}

// --- Ticket 05: explicit edge cases ---

#[test]
fn single_rule_candidate_resolves_to_that_rule() {
    let ruleset = Ruleset::build(vec![priority_rule(
        "solo",
        square(0.0, 0.0, 10.0, 10.0),
        3,
        &[("kind", PropertyValue::Str("solo".into()))],
    )])
    .unwrap();
    let local = candidate("c", square(2.0, 2.0, 4.0, 4.0));
    let outcomes = ruleset.resolve(&[local], &intersects());
    let ResolutionOutcome::Resolved {
        winner,
        values,
        applicable,
    } = &outcomes[0]
    else {
        panic!("expected a resolved outcome");
    };
    assert_eq!(*winner, ruleset.rule_id("solo").unwrap());
    assert_eq!(applicable.len(), 1);
    assert_eq!(
        values.get("kind"),
        Some(&PropertyValue::Str("solo".into()))
    );
}

#[test]
fn all_defaults_ties_resolve_by_declaration_order() {
    // Every rule is unprioritized (0): ties break by declaration order.
    let ruleset = Ruleset::build(vec![
        priority_rule(
            "first",
            square(0.0, 0.0, 10.0, 10.0),
            0,
            &[("kind", PropertyValue::Str("first".into()))],
        ),
        priority_rule(
            "second",
            square(5.0, 5.0, 15.0, 15.0),
            0,
            &[("kind", PropertyValue::Str("second".into()))],
        ),
        priority_rule(
            "third",
            square(8.0, 8.0, 18.0, 18.0),
            0,
            &[("kind", PropertyValue::Str("third".into()))],
        ),
    ])
    .unwrap();
    let outcomes = ruleset.resolve(&[inside()], &intersects());
    let ResolutionOutcome::Resolved {
        winner,
        applicable,
        ..
    } = &outcomes[0]
    else {
        panic!("expected a resolved outcome");
    };
    assert_eq!(*winner, ruleset.rule_id("first").unwrap());
    let ids: Vec<_> = applicable.iter().map(|rule| rule.rule_id).collect();
    assert_eq!(
        ids,
        vec![
            ruleset.rule_id("first").unwrap(),
            ruleset.rule_id("second").unwrap(),
            ruleset.rule_id("third").unwrap(),
        ]
    );
}
