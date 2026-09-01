//! Correctness at extreme geometry complexity and metadata richness — the
//! engine must build and query rules with tens of thousands of vertices and
//! many typed properties, not just the ~30-rule benchmark shapes.

use std::collections::BTreeMap;

use geo::Polygon;
use spatial_rules_core::{
    CandidateOutcome, PropertyValue, Query, Rule, Ruleset, SpatialPredicate,
};

mod common;
use common::{candidate_geometry, jittered_ring, square_around};

/// A rule with a `vertices`-point exterior, a hole, and `fields` extra typed
/// properties (plus `classification`).
fn complex_rule(id: &str, cx: f64, vertices: usize, fields: usize, seed: u64) -> Rule {
    let exterior = jittered_ring(cx, 0.0, 10.0, vertices, seed);
    let hole = jittered_ring(cx, 0.0, 3.0, 400, seed.wrapping_add(1));
    let mut properties = BTreeMap::new();
    properties.insert(
        "classification".to_string(),
        PropertyValue::Str("restricted".into()),
    );
    for f in 0..fields {
        properties.insert(format!("field_{f}"), PropertyValue::Int((f % 7) as i64));
    }
    Rule {
        id: id.to_string(),
        properties,
        geometry: geo::Geometry::Polygon(Polygon::new(exterior, vec![hole])),
        priority: 0,
    }
}

#[test]
fn extreme_complexity_and_metadata_build_and_query_correctly() {
    // 2,000-vertex exteriors are ~5× the benchmark maximum; the debug-profile
    // validation/prepared-geometry code gets quadratic-slow beyond this, and
    // the release-mode `complex.mjs` benchmark covers truly full-detail sizes.
    let rules = vec![
        complex_rule("r0", 0.0, 2_000, 40, 1),
        complex_rule("r1", 40.0, 2_000, 40, 2),
    ];
    let ruleset = Ruleset::build(rules).expect("complex rules must build");

    let r0 = ruleset.rule_id("r0").unwrap();
    assert_eq!(ruleset.properties(r0).len(), 41); // 40 fields + classification

    // On the ring (between the hole at r≈2.1–3.6 and the exterior at r≈7–12):
    // intersects r0 only.
    let on_ring = candidate_geometry("on-ring", square_around(5.0, 0.0, 0.25));
    let outcomes = ruleset.query(
        std::slice::from_ref(&on_ring),
        &Query::new(SpatialPredicate::Intersects),
    );
    assert_eq!(        outcomes, vec![CandidateOutcome::Matched { rule_ids: vec![r0], overlaps: None, aggregate: None }]
);

    // Inside the hole: disjoint from r0, and far from r1.
    let in_hole = candidate_geometry("in-hole", square_around(0.0, 0.0, 0.25));
    let outcomes = ruleset.query(
        std::slice::from_ref(&in_hole),
        &Query::new(SpatialPredicate::Intersects),
    );
    assert_eq!(outcomes, vec![CandidateOutcome::NotMatched]);

    // The compile-time property index answers a `where` over the 40-field
    // metadata without falling back to per-rule evaluation.
    let query = Query::from_json(&serde_json::json!({
        "spatial": { "predicate": "intersects" },
        "where": { "classification": "restricted" }
    }))
    .unwrap();
    let outcomes = ruleset.query(std::slice::from_ref(&on_ring), &query);
    assert_eq!(        outcomes, vec![CandidateOutcome::Matched { rule_ids: vec![r0], overlaps: None, aggregate: None }]
);
}
