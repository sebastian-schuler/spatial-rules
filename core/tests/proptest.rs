//! Property-based tests (ticket 07): DE-9IM invariants, `WhereExpr` evaluation
//! totality, and batch-alignment invariants on randomly generated inputs.
//!
//! These never recompute expected values with the code under test — each
//! property is an independent identity from the DE-9IM spec (ADR-0008) or the
//! documented result model (ADR-0004).

use std::collections::{BTreeMap, HashSet};

use geo::{Coord, Relate, Rect};
use proptest::prelude::*;
use serde_json::json;
use spatial_rules_core::{
    Candidate, CandidateOutcome, PropertyValue, Query, ResolutionOutcome, Rule, Ruleset,
    SpatialIndexKind, SpatialPredicate, WhereExpr,
};

/// A non-degenerate integer-aligned rectangle (always a valid simple polygon),
/// so DE-9IM predicates are well-defined on every generated pair.
fn rect_strategy() -> impl Strategy<Value = Rect<f64>> {
    (-100i32..100, 1i32..100, -100i32..100, 1i32..100).prop_map(
        |(min_x, width, min_y, height)| {
            Rect::new(
                Coord {
                    x: min_x as f64,
                    y: min_y as f64,
                },
                Coord {
                    x: (min_x + width) as f64,
                    y: (min_y + height) as f64,
                },
            )
        },
    )
}

fn rect_to_geometry(rect: Rect<f64>) -> geo::Geometry<f64> {
    geo::Geometry::Polygon(geo::Polygon::from(rect))
}

/// Build one candidate per generated rect, in order — the shared batch-input
/// idiom of the property tests.
fn candidates_from_rects(rects: &[Rect<f64>]) -> Vec<Candidate> {
    rects
        .iter()
        .enumerate()
        .map(|(index, rect)| Candidate::new(format!("c{index}"), rect_to_geometry(*rect)))
        .collect()
}

proptest! {
    #[test]
    fn de9im_invariants_hold_on_rectangles(a in rect_strategy(), b in rect_strategy()) {
        let ab = a.relate(&b);
        let ba = b.relate(&a);

        // intersects ⇔ ¬disjoint (ADR-0008).
        prop_assert_eq!(ab.is_intersects(), !ab.is_disjoint());
        // contains → intersects (ADR-0008).
        prop_assert!(!ab.is_contains() || ab.is_intersects());
        // within(a,b) ⇔ contains(b,a): transposes (ADR-0008).
        prop_assert_eq!(ab.is_within(), ba.is_contains());
        // covers(a,b) ⇔ covered_by(b,a): transposes (ADR-0012).
        prop_assert_eq!(ab.is_covers(), ba.is_coveredby());
        // touches and overlaps are symmetric (ADR-0012).
        prop_assert_eq!(ab.is_touches(), ba.is_touches());
        prop_assert_eq!(ab.is_overlaps(), ba.is_overlaps());
    }
}

fn property_value_strategy() -> impl Strategy<Value = PropertyValue> {
    prop_oneof![
        Just(PropertyValue::Null),
        any::<bool>().prop_map(PropertyValue::Bool),
        (-1000i64..1000).prop_map(PropertyValue::Int),
        (-1000.0f64..1000.0).prop_map(PropertyValue::Float),
        any::<String>().prop_map(PropertyValue::Str),
    ]
}

fn property_map_strategy() -> impl Strategy<Value = BTreeMap<String, PropertyValue>> {
    let key = prop::sample::select(vec![
        "active".to_string(),
        "priority".to_string(),
        "country".to_string(),
        "classification".to_string(),
    ]);
    prop::collection::btree_map(key, property_value_strategy(), 0..6)
}

/// A fixed suite covering every `where` operator, evaluated against random
/// typed property maps.
fn sample_where_exprs() -> Vec<WhereExpr> {
    let clauses = [
        json!({ "active": true }),
        json!({ "active": { "$eq": true } }),
        json!({ "active": { "$ne": true } }),
        json!({ "active": { "$not": { "$eq": false } } }),
        json!({ "active": { "$exists": true } }),
        json!({ "priority": { "$gt": 5 } }),
        json!({ "priority": { "$gte": 5 } }),
        json!({ "priority": { "$lt": 5 } }),
        json!({ "priority": { "$lte": 5 } }),
        json!({ "priority": { "$in": [1, 2, 3] } }),
        json!({ "priority": { "$nin": [1, 2, 3] } }),
        json!({ "country": "HR" }),
        json!({ "$and": [{ "active": true }, { "priority": { "$gt": 0 } }] }),
        json!({ "$or": [{ "country": "HR" }, { "country": "SI" }] }),
    ];
    clauses
        .iter()
        .map(|value| WhereExpr::parse(value).unwrap())
        .collect()
}

proptest! {
    #[test]
    fn where_eval_is_total_on_random_properties(props in property_map_strategy()) {
        // `eval` must be total: a bool for every property map, never a panic,
        // under the documented missing/type-mismatch = non-match rule.
        for expr in sample_where_exprs() {
            let _ = expr.eval(&props);
        }
    }

    #[test]
    fn batch_mask_aligns_to_input(
        rules in prop::collection::vec(rect_strategy(), 1..10),
        candidates in prop::collection::vec(rect_strategy(), 1..20),
    ) {
        let rules: Vec<Rule> = rules
            .iter()
            .enumerate()
            .map(|(index, rect)| Rule {
                id: format!("r{index}"),
                properties: BTreeMap::new(),
                geometry: rect_to_geometry(*rect),
                priority: 0,
            })
            .collect();
        let ruleset = Ruleset::build(rules).unwrap();

        let candidates = candidates_from_rects(&candidates);

        let query = Query::new(SpatialPredicate::Intersects);

        // The mask is aligned to input order and binary (valid rect candidates
        // are never `Invalid`, ADR-0004).
        let mask = ruleset.query_mask(&candidates, &query);
        prop_assert_eq!(mask.len(), candidates.len());
        for value in &mask {
            prop_assert!(*value == 0 || *value == 1);
        }

        // The rich path is aligned to input order too.
        let outcomes = ruleset.query(&candidates, &query);
        prop_assert_eq!(outcomes.len(), candidates.len());
    }
}

// --- Ticket 05: resolution invariants (ADR-0015) ---

/// Precedence strategy: non-negative priorities, so `0` plays the
/// "unprioritized" slot and the ADR's "unprioritized rules sort below any
/// explicit priority" invariant is well-defined.
fn priority_strategy() -> impl Strategy<Value = i64> {
    0i64..6
}

proptest! {
    #[test]
    fn resolution_invariants_hold_on_random_rulesets(
        rule_data in prop::collection::vec(
            (rect_strategy(), priority_strategy(), property_map_strategy()),
            1..8,
        ),
        candidates in prop::collection::vec(rect_strategy(), 1..4),
    ) {
        let rules: Vec<Rule> = rule_data
            .iter()
            .enumerate()
            .map(|(index, (rect, priority, properties))| Rule {
                id: format!("r{index}"),
                properties: properties.clone(),
                geometry: rect_to_geometry(*rect),
                priority: *priority,
            })
            .collect();
        let ruleset = Ruleset::build(rules.clone()).unwrap();
        let scan = Ruleset::build_with(rules, SpatialIndexKind::LinearScan).unwrap();

        let candidates = candidates_from_rects(&candidates);

        let query = Query::new(SpatialPredicate::Intersects);
        let outcomes = ruleset.resolve(&candidates, &query);

        // Determinism: repeat evaluation is byte-identical.
        let again = ruleset.resolve(&candidates, &query);
        prop_assert_eq!(&outcomes, &again);

        // The linear-scan index kind resolves identically to rstar.
        let scanned = scan.resolve(&candidates, &query);
        prop_assert_eq!(&outcomes, &scanned);

        // The applicable set is exactly the set the match path reports —
        // ADR-0015's "the same set `rule_ids` reports today" — an independent
        // oracle that catches a rule wrongly dropped from resolution.
        let matches = ruleset.query(&candidates, &query);
        for (resolution, matched) in outcomes.iter().zip(matches.iter()) {
            match (resolution, matched) {
                (
                    ResolutionOutcome::Resolved { applicable, .. },
                    CandidateOutcome::Matched { rule_ids, .. },
                ) => {
                    let mut resolved_ids: Vec<_> =
                        applicable.iter().map(|rule| rule.rule_id).collect();
                    let mut matched_ids = rule_ids.clone();
                    resolved_ids.sort();
                    matched_ids.sort();
                    prop_assert_eq!(
                        &resolved_ids,
                        &matched_ids,
                        "the applicable set must equal the match path's rule ids"
                    );
                }
                (ResolutionOutcome::NotMatched, CandidateOutcome::NotMatched) => {}
                (ResolutionOutcome::Invalid { .. }, CandidateOutcome::Invalid { .. }) => {}
                (resolution, matched) => prop_assert!(
                    false,
                    "resolution and match outcomes must agree: {resolution:?} vs {matched:?}"
                ),
            }
        }

        for outcome in &outcomes {
            let ResolutionOutcome::Resolved { winner, values, applicable } = outcome else {
                continue;
            };

            // The applicable set is ordered: priority desc, ties by declaration
            // order (ascending rule id).
            prop_assert!(!applicable.is_empty());
            for pair in applicable.windows(2) {
                prop_assert!(
                    pair[0].priority > pair[1].priority
                        || (pair[0].priority == pair[1].priority
                            && pair[0].rule_id < pair[1].rule_id),
                    "applicable must be priority desc, then declaration order"
                );
            }

            // The winner is the head of the ordered set (ADR-0015). Pin it —
            // and the explanation's priority fields — against the ruleset's
            // authoritative hoisted priorities, not the applicable records the
            // implementation filled in, so a wrong-priority bug is caught even
            // if it also corrupted the explanation.
            prop_assert_eq!(*winner, applicable[0].rule_id);
            for rule in applicable {
                prop_assert_eq!(
                    rule.priority,
                    ruleset.priority(rule.rule_id),
                    "explanation priority must match the ruleset's hoisted priority"
                );
            }
            let max_priority = applicable
                .iter()
                .map(|rule| ruleset.priority(rule.rule_id))
                .max()
                .unwrap();
            prop_assert_eq!(
                ruleset.priority(*winner),
                max_priority,
                "an unprioritized (0) rule never outranks an explicitly prioritized one"
            );

            // Values are first-provider-wins over the ordered set: each field
            // takes its value from the highest-precedence applicable rule that
            // defines it; no field an applicable rule defines is dropped; no
            // invented field appears.
            for (key, value) in values {
                let provider = applicable.iter().find(|rule| {
                    ruleset.properties(rule.rule_id).contains_key(key)
                });
                prop_assert!(
                    provider.is_some(),
                    "values never carry a field no applicable rule defines (key {key})"
                );
                prop_assert_eq!(
                    ruleset.properties(provider.unwrap().rule_id).get(key),
                    Some(value)
                );
            }
            for rule in applicable {
                for key in ruleset.properties(rule.rule_id).keys() {
                    prop_assert!(values.contains_key(key), "a defined field is never dropped");
                }
            }
        }
    }

    #[test]
    fn exclude_rule_ids_removes_rules_from_resolution(
        rule_data in prop::collection::vec((rect_strategy(), priority_strategy()), 1..8),
        candidates in prop::collection::vec(rect_strategy(), 1..4),
        excluded_indices in prop::collection::vec(any::<usize>(), 0..4),
    ) {
        // Each rule defines a unique `source` property, so a rule's
        // contribution to the merged values is traceable.
        let rules: Vec<Rule> = rule_data
            .iter()
            .enumerate()
            .map(|(index, (rect, priority))| {
                let mut properties = BTreeMap::new();
                properties.insert(
                    "source".to_string(),
                    PropertyValue::Str(format!("r{index}")),
                );
                Rule {
                    id: format!("r{index}"),
                    properties,
                    geometry: rect_to_geometry(*rect),
                    priority: *priority,
                }
            })
            .collect();
        let ruleset = Ruleset::build(rules).unwrap();

        let excluded: HashSet<String> = excluded_indices
            .iter()
            .map(|&index| format!("r{}", index % ruleset.len()))
            .collect();
        let query = Query::from_json(&json!({
            "spatial": { "predicate": "intersects" },
            "excludeRuleIds": excluded.iter().cloned().collect::<Vec<_>>()
        }))
        .unwrap();

        let candidates = candidates_from_rects(&candidates);

        for outcome in ruleset.resolve(&candidates, &query) {
            let ResolutionOutcome::Resolved { winner, values, applicable, .. } = outcome else {
                continue;
            };
            // No excluded rule is applicable, and no excluded rule wins.
            for rule in &applicable {
                prop_assert!(!excluded.contains(ruleset.string_id(rule.rule_id)));
            }
            prop_assert!(!excluded.contains(ruleset.string_id(winner)));

            // Every rule defines a unique `source`, so the merged source is the
            // winner's — which the assertion above guarantees is not excluded,
            // so no excluded rule's field leaks into the values.
            prop_assert_eq!(
                values.get("source"),
                Some(&PropertyValue::Str(ruleset.string_id(winner).to_string()))
            );
        }
    }

    #[test]
    fn where_admission_agrees_with_the_match_path(
        rule_data in prop::collection::vec(
            (rect_strategy(), priority_strategy(), any::<bool>()),
            1..8,
        ),
        candidates in prop::collection::vec(rect_strategy(), 1..4),
    ) {
        // Every rule carries an `active` bool so the where clause is meaningful
        // (some rules admitted, some not).
        let rules: Vec<Rule> = rule_data
            .iter()
            .enumerate()
            .map(|(index, (rect, priority, active))| {
                let mut properties = BTreeMap::new();
                properties.insert("active".to_string(), PropertyValue::Bool(*active));
                Rule {
                    id: format!("r{index}"),
                    properties,
                    geometry: rect_to_geometry(*rect),
                    priority: *priority,
                }
            })
            .collect();
        let ruleset = Ruleset::build(rules).unwrap();
        let candidates = candidates_from_rects(&candidates);
        let query = Query::from_json(&json!({
            "spatial": { "predicate": "intersects" },
            "where": { "active": true }
        }))
        .unwrap();

        // Resolution and the match path must admit exactly the same rules: the
        // where-admitted applicable set equals the match path's rule ids.
        let resolutions = ruleset.resolve(&candidates, &query);
        let matches = ruleset.query(&candidates, &query);
        for (resolution, matched) in resolutions.iter().zip(matches.iter()) {
            match (resolution, matched) {
                (
                    ResolutionOutcome::Resolved { applicable, .. },
                    CandidateOutcome::Matched { rule_ids, .. },
                ) => {
                    let mut resolved_ids: Vec<_> =
                        applicable.iter().map(|rule| rule.rule_id).collect();
                    let mut matched_ids = rule_ids.clone();
                    resolved_ids.sort();
                    matched_ids.sort();
                    prop_assert_eq!(
                        &resolved_ids,
                        &matched_ids,
                        "where-admitted applicable set must equal the match path's rule ids"
                    );
                }
                (ResolutionOutcome::NotMatched, CandidateOutcome::NotMatched) => {}
                (ResolutionOutcome::Invalid { .. }, CandidateOutcome::Invalid { .. }) => {}
                (resolution, matched) => prop_assert!(
                    false,
                    "where admission must agree: {resolution:?} vs {matched:?}"
                ),
            }
        }
    }
}
