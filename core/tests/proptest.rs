//! Property-based tests (ticket 07): DE-9IM invariants, `WhereExpr` evaluation
//! totality, and batch-alignment invariants on randomly generated inputs.
//!
//! These never recompute expected values with the code under test — each
//! property is an independent identity from the DE-9IM spec (ADR-0008) or the
//! documented result model (ADR-0004).

use std::collections::BTreeMap;

use geo::{Coord, Relate, Rect};
use proptest::prelude::*;
use serde_json::json;
use spatial_rules_core::{
    Candidate, PropertyValue, Query, Rule, Ruleset, SpatialPredicate, WhereExpr,
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
            })
            .collect();
        let ruleset = Ruleset::build(rules).unwrap();

        let candidates: Vec<Candidate> = candidates
            .iter()
            .enumerate()
            .map(|(index, rect)| Candidate {
                id: format!("c{index}"),
                geometry: rect_to_geometry(*rect),
            })
            .collect();

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
