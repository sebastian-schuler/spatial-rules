//! Shared rich-JSON serialization for the language bindings. The three binding
//! crates — `node` (napi), `wasm`, `python` — all emit the same
//! per-candidate outcome payloads (`ruleIds`/`winner`/`values`/`applicable`/
//! `aggregate`/`overlaps`); the serializers and the query parser live here
//! once, so the rich-outcome wire contract has a single home. The per-outcome
//! serializers are internal; the `*_rich_json` batch helpers are the interface
//! every binding crosses.

use spatial_rules_core::{
    Aggregate, CandidateOutcome, Query, ReplaceReport, ResolutionOutcome, Ruleset, SpatialError,
};

/// The `"SR_CODE: message"` string a binding throws as its error (the same
/// contract the napi async path and the wasm path use, reconstructed into a
/// coded error by the JS/Python wrapper).
pub fn spatial_error_message(error: &SpatialError) -> String {
    format!("{}: {}", error.code, error.message)
}

/// Parse the query JSON into the engine's `Query` — the same parser every
/// binding uses (`withinDistance`/`at`/`aggregate` and all).
pub fn parse_query(query_json: &str) -> Result<Query, SpatialError> {
    let value: serde_json::Value = serde_json::from_str(query_json)
        .map_err(|e| SpatialError::invalid_query(format!("query is not valid JSON: {e}")))?;
    Query::from_json(&value)
}

/// The ADR-0007 observability report as its JSON object.
pub fn report_to_json(report: ReplaceReport) -> serde_json::Value {
    serde_json::json!({
        "version": report.version,
        "ruleCount": report.rule_count,
        "buildDurationMs": report.build_duration_ms,
        "lastSwapTime": report.last_swap_time_unix_ms,
    })
}

/// The requested per-candidate aggregate as the ADR-0018 JSON object — only
/// the functions the spec asked for (and that produced a value) are emitted.
/// Internal: the outcome already carries the computed [`Aggregate`].
fn aggregate_json(aggregate: &Aggregate) -> serde_json::Value {
    let mut object = serde_json::Map::new();
    if let Some(count) = aggregate.count {
        object.insert("count".to_string(), serde_json::json!(count));
    }
    for (key, value) in [
        ("min", aggregate.min),
        ("max", aggregate.max),
        ("sum", aggregate.sum),
        ("avg", aggregate.avg),
        ("coverage", aggregate.coverage),
    ] {
        if let Some(value) = value {
            object.insert(key.to_string(), serde_json::json!(value));
        }
    }
    serde_json::Value::Object(object)
}

/// One `ResolutionOutcome` as the ADR-0015 JSON shape: `{outcome, winner,
/// values, applicable, aggregate}` for resolved, `{outcome: notMatched}`, or
/// `{outcome: invalid, reason}`. Rule ids are the application's original
/// strings; `values` uses the rules' compact typed properties; `aggregate` is
/// the outcome's precomputed analytics (ADR-0018), absent when not requested.
/// Internal: [`resolve_rich_json`] assembles the batch from these.
fn resolution_outcome_to_json(
    ruleset: &Ruleset,
    outcome: &ResolutionOutcome,
) -> serde_json::Value {
    match outcome {
        ResolutionOutcome::NotMatched => serde_json::json!({ "outcome": "notMatched" }),
        ResolutionOutcome::Invalid { reason } => {
            serde_json::json!({ "outcome": "invalid", "reason": reason })
        }
        ResolutionOutcome::Resolved {
            winner,
            values,
            applicable,
            aggregate,
        } => {
            let applicable_json: Vec<serde_json::Value> = applicable
                .iter()
                .map(|rule| {
                    serde_json::json!({
                        "ruleId": ruleset.string_id(rule.rule_id),
                        "priority": rule.priority,
                        "spatialMatched": rule.spatial_matched,
                        "propertyMatched": rule.property_matched,
                    })
                })
                .collect();
            let mut object = serde_json::Map::new();
            object.insert("outcome".to_string(), serde_json::json!("resolved"));
            object.insert(
                "winner".to_string(),
                serde_json::json!(ruleset.string_id(*winner)),
            );
            let mut values_json = serde_json::Map::new();
            for (key, value) in values {
                values_json.insert(
                    key.clone(),
                    serde_json::to_value(value)
                        .expect("property values always serialize to JSON scalars"),
                );
            }
            object.insert("values".to_string(), serde_json::Value::Object(values_json));
            object.insert(
                "applicable".to_string(),
                serde_json::Value::Array(applicable_json),
            );
            if let Some(aggregate) = aggregate {
                object.insert("aggregate".to_string(), aggregate_json(aggregate));
            }
            serde_json::Value::Object(object)
        }
    }
}

/// One `CandidateOutcome` as the ADR-0004 JSON shape, with the outcome's
/// `overlaps`/`aggregate` payloads attached (ADR-0012/0018).
/// Internal: [`query_rich_json`] assembles the batch from these.
fn candidate_outcome_to_json(
    ruleset: &Ruleset,
    outcome: &CandidateOutcome,
) -> serde_json::Value {
    match outcome {
        CandidateOutcome::NotMatched => serde_json::json!({ "outcome": "notMatched" }),
        CandidateOutcome::Invalid { reason } => {
            serde_json::json!({ "outcome": "invalid", "reason": reason })
        }
        CandidateOutcome::Matched {
            rule_ids,
            overlaps,
            aggregate,
        } => {
            let ids: Vec<&str> = rule_ids
                .iter()
                .map(|id| ruleset.string_id(*id))
                .collect();
            let mut object = serde_json::Map::new();
            object.insert("outcome".to_string(), serde_json::json!("matched"));
            object.insert("ruleIds".to_string(), serde_json::json!(ids));
            if let Some(overlaps) = overlaps {
                let per_rule: Vec<serde_json::Value> = rule_ids
                    .iter()
                    .zip(overlaps)
                    .map(|(id, metric)| {
                        serde_json::json!({
                            "ruleId": ruleset.string_id(*id),
                            "overlapArea": metric.overlap_area,
                            "overlapRatio": metric.overlap_ratio,
                        })
                    })
                    .collect();
                object.insert("overlaps".to_string(), serde_json::Value::Array(per_rule));
            }
            if let Some(aggregate) = aggregate {
                object.insert("aggregate".to_string(), aggregate_json(aggregate));
            }
            serde_json::Value::Object(object)
        }
    }
}

/// Assemble a whole batch of `CandidateOutcome`s (in input order) into the
/// JSON string a binding hands off — the wire contract (ADR-0004/0012/0018).
/// The outcomes are self-contained (rule ids, overlaps, aggregate), so the
/// candidate and query are not needed here. The payloads are built from domain
/// types that always serialize, so the call is infallible.
pub fn query_rich_json(ruleset: &Ruleset, outcomes: &[CandidateOutcome]) -> String {
    let rich: Vec<serde_json::Value> = outcomes
        .iter()
        .map(|outcome| candidate_outcome_to_json(ruleset, outcome))
        .collect();
    serde_json::to_string(&rich).expect("candidate outcome payloads are always JSON-serializable")
}

/// Assemble a whole batch of `ResolutionOutcome`s (in input order) into the
/// JSON string a binding hands off — the wire contract (ADR-0015/0018).
/// Internal per-outcome serialization is reused; the batch helper is the
/// interface callers use. Infallible for the same reason as
/// [`query_rich_json`].
pub fn resolve_rich_json(ruleset: &Ruleset, outcomes: &[ResolutionOutcome]) -> String {
    let rich: Vec<serde_json::Value> = outcomes
        .iter()
        .map(|outcome| resolution_outcome_to_json(ruleset, outcome))
        .collect();
    serde_json::to_string(&rich).expect("resolution outcome payloads are always JSON-serializable")
}

#[cfg(test)]
mod tests {
    use super::*;
    use spatial_rules_core::{candidates_from_geojson, Query, SpatialPredicate};

    const RULES: &str = r#"{"type":"FeatureCollection","features":[
        {"type":"Feature","id":"zone-a","properties":{"speedLimit":30},"geometry":{"type":"Polygon","coordinates":[[[0,0],[0,10],[10,10],[10,0],[0,0]]]}},
        {"type":"Feature","id":"zone-b","properties":{"speedLimit":50},"geometry":{"type":"Polygon","coordinates":[[[2,2],[2,12],[12,12],[12,2],[2,2]]]}}
    ]}"#;
    const CANDIDATES: &str = r#"{"type":"FeatureCollection","features":[
        {"type":"Feature","id":"c","properties":{},"geometry":{"type":"Polygon","coordinates":[[[2,2],[2,4],[4,4],[4,2],[2,2]]]}},
        {"type":"Feature","id":"far","properties":{},"geometry":{"type":"Polygon","coordinates":[[[50,50],[50,60],[60,60],[60,50],[50,50]]]}}
    ]}"#;
    const INVALID_CANDIDATES: &str = r#"{"type":"FeatureCollection","features":[
        {"type":"Feature","id":"bad","properties":{},"geometry":{"type":"Polygon","coordinates":[[[0,0],[10,10],[0,10],[10,0],[0,0]]]}}
    ]}"#;

    fn ruleset() -> Ruleset {
        Ruleset::from_geojson(RULES).unwrap()
    }

    fn parsed(json: &str) -> serde_json::Value {
        serde_json::from_str(json).unwrap()
    }

    #[test]
    fn report_serializes_the_observability_fields() {
        let report = ReplaceReport {
            version: 3,
            rule_count: 30,
            build_duration_ms: 12,
            last_swap_time_unix_ms: 1234,
        };
        assert_eq!(
            report_to_json(report),
            serde_json::json!({
                "version": 3,
                "ruleCount": 30,
                "buildDurationMs": 12,
                "lastSwapTime": 1234,
            })
        );
    }

    #[test]
    fn query_rich_json_serializes_string_rule_ids_without_overlap_by_default() {
        let ruleset = ruleset();
        let candidates = candidates_from_geojson(CANDIDATES).unwrap();
        let query = Query::new(SpatialPredicate::Intersects);
        let outcomes = ruleset.query(&candidates, &query);

        let parsed = parsed(&query_rich_json(&ruleset, &outcomes));
        let array = parsed.as_array().unwrap();
        assert_eq!(array.len(), 2);
        assert_eq!(array[0]["outcome"], serde_json::json!("matched"));
        assert_eq!(array[0]["ruleIds"], serde_json::json!(["zone-a", "zone-b"]));
        // Overlap metrics are absent unless includeOverlap was requested.
        assert!(array[0].get("overlaps").is_none());
        assert_eq!(array[1]["outcome"], serde_json::json!("notMatched"));
    }

    #[test]
    fn query_rich_json_attaches_overlap_metrics_when_requested() {
        let ruleset = ruleset();
        let candidates = candidates_from_geojson(CANDIDATES).unwrap();
        let query = Query::new(SpatialPredicate::Intersects).with_overlap();
        let outcomes = ruleset.query(&candidates, &query);

        let parsed = parsed(&query_rich_json(&ruleset, &outcomes));
        let matches = parsed[0]["overlaps"].as_array().unwrap();
        assert_eq!(matches.len(), 2);
        for overlap in matches {
            assert!(overlap["overlapArea"].is_number());
            assert!(overlap["overlapRatio"].is_number());
        }
        // notMatched carries no overlaps payload.
        assert!(parsed[1].get("overlaps").is_none());
    }

    #[test]
    fn query_rich_json_attaches_aggregate_when_requested() {
        let ruleset = ruleset();
        let candidates = candidates_from_geojson(CANDIDATES).unwrap();
        let query = Query::from_json(&serde_json::json!({
            "spatial": { "predicate": "intersects" },
            "aggregate": { "count": true, "min": "speedLimit" }
        }))
        .unwrap();
        let outcomes = ruleset.query(&candidates, &query);

        let parsed = parsed(&query_rich_json(&ruleset, &outcomes));
        assert_eq!(parsed[0]["aggregate"]["count"], serde_json::json!(2));
        assert_eq!(parsed[0]["aggregate"]["min"], serde_json::json!(30.0));
        // notMatched carries no aggregate.
        assert!(parsed[1].get("aggregate").is_none());
    }

    #[test]
    fn query_rich_json_serializes_invalid_candidates() {
        let ruleset = ruleset();
        let candidates = candidates_from_geojson(INVALID_CANDIDATES).unwrap();
        let query = Query::new(SpatialPredicate::Intersects);
        let outcomes = ruleset.query(&candidates, &query);

        let parsed = parsed(&query_rich_json(&ruleset, &outcomes));
        assert_eq!(parsed[0]["outcome"], serde_json::json!("invalid"));
        assert!(parsed[0]["reason"].is_string());
    }

    #[test]
    fn query_rich_json_empty_batch_serializes_empty_array() {
        let ruleset = ruleset();
        let json = query_rich_json(&ruleset, &[]);
        assert_eq!(json, "[]");
    }

    #[test]
    fn resolve_rich_json_serializes_winner_values_and_applicable() {
        let ruleset = ruleset();
        let candidates = candidates_from_geojson(CANDIDATES).unwrap();
        let query = Query::new(SpatialPredicate::Intersects);
        let outcomes = ruleset.resolve(&candidates, &query);

        let parsed = parsed(&resolve_rich_json(&ruleset, &outcomes));
        let array = parsed.as_array().unwrap();
        assert_eq!(array.len(), 2);
        assert_eq!(array[0]["outcome"], serde_json::json!("resolved"));
        // Both rules have priority 0: ties break by declaration order.
        assert_eq!(array[0]["winner"], serde_json::json!("zone-a"));
        assert_eq!(array[0]["values"]["speedLimit"], serde_json::json!(30));
        assert_eq!(array[0]["applicable"][0]["ruleId"], serde_json::json!("zone-a"));
        assert_eq!(array[0]["applicable"][1]["ruleId"], serde_json::json!("zone-b"));
        assert_eq!(array[1]["outcome"], serde_json::json!("notMatched"));
    }

    #[test]
    fn resolve_rich_json_attaches_aggregate_when_requested() {
        let ruleset = ruleset();
        let candidates = candidates_from_geojson(CANDIDATES).unwrap();
        let query = Query::from_json(&serde_json::json!({
            "spatial": { "predicate": "intersects" },
            "aggregate": { "count": true }
        }))
        .unwrap();
        let outcomes = ruleset.resolve(&candidates, &query);

        let parsed = parsed(&resolve_rich_json(&ruleset, &outcomes));
        assert_eq!(parsed[0]["aggregate"]["count"], serde_json::json!(2));
        assert!(parsed[1].get("aggregate").is_none());
    }

    #[test]
    fn resolve_rich_json_serializes_invalid_candidates() {
        let ruleset = ruleset();
        let candidates = candidates_from_geojson(INVALID_CANDIDATES).unwrap();
        let query = Query::new(SpatialPredicate::Intersects);
        let outcomes = ruleset.resolve(&candidates, &query);

        let parsed = parsed(&resolve_rich_json(&ruleset, &outcomes));
        assert_eq!(parsed[0]["outcome"], serde_json::json!("invalid"));
        assert!(parsed[0]["reason"].is_string());
    }

    #[test]
    fn resolve_rich_json_empty_batch_serializes_empty_array() {
        let ruleset = ruleset();
        let json = resolve_rich_json(&ruleset, &[]);
        assert_eq!(json, "[]");
    }
}