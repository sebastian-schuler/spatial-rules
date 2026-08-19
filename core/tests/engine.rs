//! Integration tests for the `Engine`: atomic ruleset replacement and
//! concurrency (ADR-0007, ADR-0009).

use std::sync::Arc;

use spatial_rules_core::{candidates_from_geojson, Engine, Query, SpatialPredicate};

const RULE_A: &str = r#"{
  "type": "FeatureCollection",
  "features": [
    { "type": "Feature", "id": "a", "properties": {}, "geometry": { "type": "Polygon", "coordinates": [[[0, 0], [0, 10], [10, 10], [10, 0], [0, 0]]] } }
  ]
}"#;

const RULE_B: &str = r#"{
  "type": "FeatureCollection",
  "features": [
    { "type": "Feature", "id": "b", "properties": {}, "geometry": { "type": "Polygon", "coordinates": [[[100, 100], [100, 110], [110, 110], [110, 100], [100, 100]]] } }
  ]
}"#;

const INVALID_RULE: &str = r#"{
  "type": "FeatureCollection",
  "features": [
    { "type": "Feature", "id": "bad", "properties": {}, "geometry": { "type": "Polygon", "coordinates": [[[0, 0], [10, 10], [0, 10], [10, 0], [0, 0]]] } }
  ]
}"#;

const CANDIDATES: &str = r#"{
  "type": "FeatureCollection",
  "features": [
    { "type": "Feature", "id": "inside-a", "properties": {}, "geometry": { "type": "Polygon", "coordinates": [[[2, 2], [2, 4], [4, 4], [4, 2], [2, 2]]] } },
    { "type": "Feature", "id": "inside-b", "properties": {}, "geometry": { "type": "Polygon", "coordinates": [[[102, 102], [102, 104], [104, 104], [104, 102], [102, 102]]] } }
  ]
}"#;

fn intersects() -> Query {
    Query::new(SpatialPredicate::Intersects)
}

fn matched_count(outcomes: &[spatial_rules_core::CandidateOutcome]) -> usize {
    outcomes
        .iter()
        .filter(|outcome| matches!(outcome, spatial_rules_core::CandidateOutcome::Matched { .. }))
        .count()
}

#[test]
fn replace_swaps_ruleset_and_reports_observability() {
    let engine = Engine::from_geojson(RULE_A).unwrap();
    let candidates = candidates_from_geojson(CANDIDATES).unwrap();
    assert_eq!(engine.current().version, 1);
    assert_eq!(engine.current().rule_count, 1);

    // Rule A active: only "inside-a" matches.
    let outcomes = engine.query(&candidates, &intersects());
    assert_eq!(matched_count(&outcomes), 1);

    let report = engine.replace_from_geojson(RULE_B).unwrap();
    assert_eq!(report.version, 2);
    assert_eq!(report.rule_count, 1);
    assert_eq!(engine.current().version, 2);

    // Rule B active: only "inside-b" matches.
    let outcomes = engine.query(&candidates, &intersects());
    assert_eq!(matched_count(&outcomes), 1);
    // The single match is now the second candidate.
    assert!(matches!(outcomes[1], spatial_rules_core::CandidateOutcome::Matched { .. }));
    assert!(matches!(outcomes[0], spatial_rules_core::CandidateOutcome::NotMatched));
}

#[test]
fn repeated_replacement_versions_increment() {
    let engine = Engine::from_geojson(RULE_A).unwrap();
    for expected in 2..=5 {
        let report = engine.replace_from_geojson(if expected % 2 == 0 { RULE_B } else { RULE_A }).unwrap();
        assert_eq!(report.version, expected);
    }
    assert_eq!(engine.current().version, 5);
}

#[test]
fn old_ruleset_snapshot_stays_alive_after_replace() {
    let engine = Engine::from_geojson(RULE_A).unwrap();
    let old = engine.snapshot();
    engine.replace_from_geojson(RULE_B).unwrap();

    // The engine has released its reference to the old ruleset; only our
    // snapshot keeps it alive.
    assert_eq!(Arc::strong_count(&old), 1);

    // The old snapshot still answers with the old ruleset.
    let candidates = candidates_from_geojson(CANDIDATES).unwrap();
    let outcomes = old.query(&candidates, &intersects());
    assert_eq!(matched_count(&outcomes), 1);
    assert!(matches!(outcomes[0], spatial_rules_core::CandidateOutcome::Matched { .. }));
}

#[test]
fn replace_with_invalid_rules_fails_and_keeps_old() {
    let engine = Engine::from_geojson(RULE_A).unwrap();
    let err = engine.replace_from_geojson(INVALID_RULE).unwrap_err();
    assert_eq!(err.code, spatial_rules_core::ErrorCode::InvalidGeometry);

    // Old ruleset is still active and observable.
    assert_eq!(engine.current().version, 1);
    let candidates = candidates_from_geojson(CANDIDATES).unwrap();
    let outcomes = engine.query(&candidates, &intersects());
    assert_eq!(matched_count(&outcomes), 1);
    assert!(matches!(outcomes[0], spatial_rules_core::CandidateOutcome::Matched { .. }));
}

#[test]
fn concurrent_queries_survive_replacement() {
    let engine = Arc::new(Engine::from_geojson(RULE_A).unwrap());
    let candidates = Arc::new(candidates_from_geojson(CANDIDATES).unwrap());
    let query = Arc::new(intersects());

    let mut handles = Vec::new();
    for _ in 0..4 {
        let engine = Arc::clone(&engine);
        let candidates = Arc::clone(&candidates);
        let query = Arc::clone(&query);
        handles.push(std::thread::spawn(move || {
            for _ in 0..200 {
                let outcomes = engine.query(candidates.as_slice(), &query);
                assert_eq!(outcomes.len(), candidates.len());
            }
        }));
    }

    // Replace repeatedly while the queries run.
    for index in 0..20 {
        engine
            .replace_from_geojson(if index % 2 == 0 { RULE_B } else { RULE_A })
            .unwrap();
    }

    for handle in handles {
        handle.join().unwrap();
    }
    assert_eq!(engine.current().version, 21);
}

#[test]
fn long_running_mixed_workload_stays_consistent() {
    let engine = Engine::from_geojson(RULE_A).unwrap();
    let candidates = candidates_from_geojson(CANDIDATES).unwrap();

    for index in 0..50 {
        let outcomes = engine.query(&candidates, &intersects());
        assert_eq!(outcomes.len(), candidates.len());
        if index % 5 == 0 {
            engine
                .replace_from_geojson(if index % 10 == 0 { RULE_B } else { RULE_A })
                .unwrap();
        }
    }
    // Final state is well-defined.
    assert!(engine.current().version >= 5);
    assert_eq!(engine.current().rule_count, 1);
}

#[test]
fn cached_preparation_survives_repeated_queries_and_invalidates_on_replace() {
    let engine = Engine::from_geojson(RULE_A).unwrap();
    let candidates = candidates_from_geojson(CANDIDATES).unwrap();

    // The first query warms the per-thread prepared-geometry cache; the
    // repeats hit it. Results must agree every time.
    for _ in 0..3 {
        let outcomes = engine.query(&candidates, &intersects());
        assert_eq!(matched_count(&outcomes), 1);
        assert!(matches!(
            outcomes[0],
            spatial_rules_core::CandidateOutcome::Matched { .. }
        ));
    }

    // Replacement must invalidate the cache: the same thread now prepares
    // RULE_B's geometry, not a stale RULE_A.
    engine.replace_from_geojson(RULE_B).unwrap();
    let outcomes = engine.query(&candidates, &intersects());
    assert_eq!(matched_count(&outcomes), 1);
    assert!(matches!(
        outcomes[0],
        spatial_rules_core::CandidateOutcome::NotMatched
    ));
    assert!(matches!(
        outcomes[1],
        spatial_rules_core::CandidateOutcome::Matched { .. }
    ));
}

#[test]
fn replace_from_canonical_swaps_and_rejects_invalid_input() {
    let engine = Engine::from_geojson(RULE_A).unwrap();
    let canonical = engine.snapshot().to_canonical().unwrap();

    // Loading the canonical form re-compiles and publishes a fresh ruleset.
    let report = engine.replace_from_canonical(&canonical).unwrap();
    assert_eq!(report.version, 2);
    assert_eq!(report.rule_count, 1);

    // A failed load leaves the old ruleset untouched (ADR-0013).
    let err = engine.replace_from_canonical(b"not json").unwrap_err();
    assert_eq!(err.code, spatial_rules_core::ErrorCode::InvalidGeoJson);
    assert_eq!(engine.current().version, 2);

    let candidates = candidates_from_geojson(CANDIDATES).unwrap();
    let outcomes = engine.query(&candidates, &intersects());
    assert_eq!(matched_count(&outcomes), 1);
}

#[test]
fn engine_applies_whole_clause_nor() {
    let engine = Engine::from_geojson(
        r#"{
          "type": "FeatureCollection",
          "features": [
            { "type": "Feature", "id": "a", "properties": { "active": true, "zone": "red" }, "geometry": { "type": "Polygon", "coordinates": [[[0, 0], [0, 10], [10, 10], [10, 0], [0, 0]]] } }
          ]
        }"#,
    )
    .unwrap();
    let candidates = candidates_from_geojson(CANDIDATES).unwrap();
    // NOT(zone = blue OR active = false): both are false for rule a, so the
    // candidate matches — $nor flows through the engine path.
    let query = Query::from_json(&serde_json::json!({
        "spatial": { "predicate": "intersects" },
        "where": { "$nor": [{ "zone": "blue" }, { "active": false }] }
    }))
    .unwrap();
    assert_eq!(matched_count(&engine.query(&candidates, &query)), 1);
}

#[test]
fn engine_accepts_point_candidates() {
    let engine = Engine::from_geojson(RULE_A).unwrap();
    let candidates = candidates_from_geojson(
        r#"{
          "type": "FeatureCollection",
          "features": [
            { "type": "Feature", "id": "pt-inside", "properties": {}, "geometry": { "type": "Point", "coordinates": [5.0, 5.0] } },
            { "type": "Feature", "id": "pt-outside", "properties": {}, "geometry": { "type": "Point", "coordinates": [50.0, 50.0] } }
          ]
        }"#,
    )
    .unwrap();
    let outcomes = engine.query(&candidates, &intersects());
    assert_eq!(matched_count(&outcomes), 1);
}
