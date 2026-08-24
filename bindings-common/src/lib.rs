//! Shared rich-JSON serialization for the wasm/python bindings
//! (`.scratch/wasm`). The three binding crates — `node` (napi, out of scope),
//! `wasm`, `python` — all emit the same per-candidate outcome payloads
//! (`ruleIds`/`winner`/`values`/`applicable`/`aggregate`/`overlaps`); the
//! serializers and the query parser live here once, instead of once per
//! binding. Node's inline copy stays in `node/src/lib.rs` (its crate is out of
//! scope for this effort).

use spatial_rules_core::{
    AggregateSpec, Candidate, CandidateOutcome, Query, ReplaceReport, ResolutionOutcome, RuleId,
    Ruleset, SpatialError,
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
pub fn aggregate_json(
    spec: &AggregateSpec,
    candidate: &Candidate,
    applicable: &[RuleId],
    ruleset: &Ruleset,
) -> serde_json::Value {
    let aggregate = spec.compute(candidate, applicable, ruleset);
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
/// values, applicable}` for resolved, `{outcome: notMatched}`, or
/// `{outcome: invalid, reason}`. Rule ids are the application's original
/// strings; `values` uses the rules' compact typed properties.
pub fn resolution_outcome_to_json(
    ruleset: &Ruleset,
    candidate: &Candidate,
    aggregate: Option<&AggregateSpec>,
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
            if let Some(spec) = aggregate {
                let rule_ids: Vec<RuleId> = applicable.iter().map(|rule| rule.rule_id).collect();
                object.insert(
                    "aggregate".to_string(),
                    aggregate_json(spec, candidate, &rule_ids, ruleset),
                );
            }
            serde_json::Value::Object(object)
        }
    }
}

/// One `CandidateOutcome` as the ADR-0004 JSON shape, with the query's
/// `aggregate`/`includeOverlap` payloads attached (ADR-0012/0018).
pub fn candidate_outcome_to_json(
    ruleset: &Ruleset,
    candidate: &Candidate,
    query: &Query,
    outcome: &CandidateOutcome,
) -> serde_json::Value {
    match outcome {
        CandidateOutcome::NotMatched => serde_json::json!({ "outcome": "notMatched" }),
        CandidateOutcome::Invalid { reason } => {
            serde_json::json!({ "outcome": "invalid", "reason": reason })
        }
        CandidateOutcome::Matched { rule_ids, overlaps } => {
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
            if let Some(spec) = &query.aggregate {
                object.insert(
                    "aggregate".to_string(),
                    aggregate_json(spec, candidate, rule_ids, ruleset),
                );
            }
            serde_json::Value::Object(object)
        }
    }
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

    #[test]
    fn query_rich_serializes_string_rule_ids() {
        let ruleset = Ruleset::from_geojson(RULES).unwrap();
        let candidates = candidates_from_geojson(CANDIDATES).unwrap();
        let query = Query::new(SpatialPredicate::Intersects);
        let outcome = &ruleset.query(&candidates, &query)[0];
        let json = candidate_outcome_to_json(&ruleset, &candidates[0], &query, outcome);
        assert_eq!(
            json,
            serde_json::json!({ "outcome": "matched", "ruleIds": ["zone-a", "zone-b"] })
        );
        let not_matched = candidate_outcome_to_json(
            &ruleset,
            &candidates[1],
            &query,
            &ruleset.query(&candidates, &query)[1],
        );
        assert_eq!(not_matched, serde_json::json!({ "outcome": "notMatched" }));
    }

    #[test]
    fn query_rich_attaches_aggregate_when_requested() {
        let ruleset = Ruleset::from_geojson(RULES).unwrap();
        let candidates = candidates_from_geojson(CANDIDATES).unwrap();
        let query = Query::from_json(&serde_json::json!({
            "spatial": { "predicate": "intersects" },
            "aggregate": { "count": true, "min": "speedLimit" }
        }))
        .unwrap();
        let outcome = &ruleset.query(&candidates, &query)[0];
        let json = candidate_outcome_to_json(&ruleset, &candidates[0], &query, outcome);
        assert_eq!(json["aggregate"]["count"], serde_json::json!(2));
        assert_eq!(json["aggregate"]["min"], serde_json::json!(30.0));
    }

    #[test]
    fn resolve_rich_serializes_winner_values_and_applicable() {
        let ruleset = Ruleset::from_geojson(RULES).unwrap();
        let candidates = candidates_from_geojson(CANDIDATES).unwrap();
        let query = Query::new(SpatialPredicate::Intersects);
        let outcome = &ruleset.resolve(&candidates, &query)[0];
        let json = resolution_outcome_to_json(&ruleset, &candidates[0], query.aggregate.as_ref(), outcome);
        assert_eq!(json["outcome"], serde_json::json!("resolved"));
        // Both rules have priority 0: ties break by declaration order.
        assert_eq!(json["winner"], serde_json::json!("zone-a"));
        assert_eq!(json["values"]["speedLimit"], serde_json::json!(30));
        assert_eq!(json["applicable"][0]["ruleId"], serde_json::json!("zone-a"));
        assert_eq!(json["applicable"][1]["ruleId"], serde_json::json!("zone-b"));
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
}