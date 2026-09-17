//! Shared rich-JSON serialization for the language bindings. The three binding
//! crates — `node` (napi), `wasm`, `python` — all emit the same
//! per-candidate outcome payloads (`ruleIds`/`winner`/`values`/`applicable`/
//! `aggregate`/`overlaps`); the serializers and the query parser live here
//! once, so the rich-outcome wire contract has a single home. The per-outcome
//! serializers are internal; the `*_rich_json` batch helpers are the interface
//! every binding crosses.

use std::collections::BTreeMap;

use serde::{Serialize, Serializer};

use spatial_rules_core::{
    candidates_from_geojson, Aggregate, Candidate, CandidateOutcome, PropertyValue, Query,
    ReplaceReport, ResolutionOutcome, Ruleset, SpatialError,
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

/// Parse a batch's candidate GeoJSON and query together — the shared
/// candidate-ingestion + query-parse handoff every binding performs before
/// evaluation. Each binding only keeps its own host-language input conversion
/// (Node `Buffer`, WASM `&str`, Python `str | bytes | dict`) and then crosses
/// this seam with plain text.
pub fn parse_inputs(
    candidates_json: &str,
    query_json: &str,
) -> Result<(Vec<Candidate>, Query), SpatialError> {
    let candidates = candidates_from_geojson(candidates_json)?;
    let query = parse_query(query_json)?;
    Ok((candidates, query))
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

/// The requested per-candidate aggregate, streamed straight to the serializer
/// (no `serde_json::Value` DOM). Fields are emitted in the alphabetical key
/// order the old `BTreeMap`-backed object produced, so the bytes are identical
/// (perf-memory 07).
#[derive(Serialize)]
struct AggregateView {
    #[serde(skip_serializing_if = "Option::is_none")]
    avg: Option<f64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    count: Option<u32>,
    #[serde(skip_serializing_if = "Option::is_none")]
    coverage: Option<f64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    max: Option<f64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    min: Option<f64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    sum: Option<f64>,
}

impl AggregateView {
    fn new(aggregate: &Aggregate) -> Self {
        Self {
            avg: aggregate.avg,
            count: aggregate.count,
            coverage: aggregate.coverage,
            max: aggregate.max,
            min: aggregate.min,
            sum: aggregate.sum,
        }
    }
}

/// One applicable rule in the resolved explanation. Field order matches the old
/// alphabetical output: `priority`, `propertyMatched`, `ruleId`,
/// `spatialMatched`.
#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
struct ApplicableView<'a> {
    priority: i64,
    property_matched: bool,
    rule_id: &'a str,
    spatial_matched: bool,
}

/// One per-rule overlap metric: `overlapArea`, `overlapRatio`, `ruleId`.
#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
struct OverlapView<'a> {
    overlap_area: f64,
    overlap_ratio: f64,
    rule_id: &'a str,
}

/// The `resolved` payload of a `ResolutionOutcome`, streamed directly. Key
/// order matches the old alphabetical output: `aggregate`, `applicable`,
/// `outcome`, `values`, `winner`.
#[derive(Serialize)]
struct ResolvedView<'a> {
    #[serde(skip_serializing_if = "Option::is_none")]
    aggregate: Option<AggregateView>,
    applicable: Vec<ApplicableView<'a>>,
    outcome: &'static str,
    values: &'a BTreeMap<String, PropertyValue>,
    winner: &'a str,
}

/// One `ResolutionOutcome` as the ADR-0015 JSON shape: `{outcome, winner,
/// values, applicable, aggregate}` for resolved, `{outcome: notMatched}`, or
/// `{outcome: invalid, reason}`. Rule ids are the application's original
/// strings; `values` uses the rules' compact typed properties; `aggregate` is
/// the outcome's precomputed analytics (ADR-0018), absent when not requested.
/// Internal: [`resolve_rich_json`] assembles the batch from these.
fn resolution_outcome_view<'a>(
    ruleset: &'a Ruleset,
    outcome: &'a ResolutionOutcome,
) -> OutcomeView<'a> {
    match outcome {
        ResolutionOutcome::NotMatched => OutcomeView::NotMatched,
        ResolutionOutcome::Invalid { reason } => OutcomeView::Invalid(reason),
        ResolutionOutcome::Resolved {
            winner,
            values,
            applicable,
            aggregate,
        } => OutcomeView::Resolved(ResolvedView {
            aggregate: aggregate.as_ref().map(AggregateView::new),
            applicable: applicable
                .iter()
                .map(|rule| ApplicableView {
                    priority: rule.priority,
                    property_matched: rule.property_matched,
                    rule_id: ruleset
                        .string_id(rule.rule_id)
                        .expect("rule id minted by this ruleset"),
                    spatial_matched: rule.spatial_matched,
                })
                .collect(),
            outcome: "resolved",
            values,
            winner: ruleset
                .string_id(*winner)
                .expect("rule id minted by this ruleset"),
        }),
    }
}

/// The `matched` payload of a `CandidateOutcome`, streamed directly. Key order
/// matches the old alphabetical output: `aggregate`, `outcome`, `overlaps`,
/// `ruleIds`.
#[derive(Serialize)]
struct MatchedView<'a> {
    #[serde(skip_serializing_if = "Option::is_none")]
    aggregate: Option<AggregateView>,
    outcome: &'static str,
    #[serde(skip_serializing_if = "Option::is_none")]
    overlaps: Option<Vec<OverlapView<'a>>>,
    #[serde(rename = "ruleIds")]
    rule_ids: Vec<&'a str>,
}

#[derive(Serialize)]
struct NotMatchedView {
    outcome: &'static str,
}

#[derive(Serialize)]
struct InvalidView<'a> {
    outcome: &'static str,
    reason: &'a str,
}

/// One outcome, chosen at runtime. Hand-written rather than
/// `#[serde(untagged)]`: an untagged enum buffers its payload through serde's
/// private `Content` type, which is precisely the DOM this path exists to
/// avoid (perf-memory 07).
enum OutcomeView<'a> {
    NotMatched,
    Invalid(&'a str),
    Matched(MatchedView<'a>),
    Resolved(ResolvedView<'a>),
}

impl Serialize for OutcomeView<'_> {
    fn serialize<S: Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        match self {
            OutcomeView::NotMatched => NotMatchedView {
                outcome: "notMatched",
            }
            .serialize(serializer),
            OutcomeView::Invalid(reason) => InvalidView {
                outcome: "invalid",
                reason,
            }
            .serialize(serializer),
            OutcomeView::Matched(view) => view.serialize(serializer),
            OutcomeView::Resolved(view) => view.serialize(serializer),
        }
    }
}

/// One `CandidateOutcome` as the ADR-0004 JSON shape, with the outcome's
/// `overlaps`/`aggregate` payloads attached (ADR-0012/0018).
/// Internal: [`query_rich_json`] assembles the batch from these.
fn candidate_outcome_view<'a>(
    ruleset: &'a Ruleset,
    outcome: &'a CandidateOutcome,
) -> OutcomeView<'a> {
    match outcome {
        CandidateOutcome::NotMatched => OutcomeView::NotMatched,
        CandidateOutcome::Invalid { reason } => OutcomeView::Invalid(reason),
        CandidateOutcome::Matched {
            rule_ids,
            overlaps,
            aggregate,
        } => OutcomeView::Matched(MatchedView {
            aggregate: aggregate.as_ref().map(AggregateView::new),
            outcome: "matched",
            overlaps: overlaps.as_ref().map(|metrics| {
                rule_ids
                    .iter()
                    .zip(metrics)
                    .map(|(id, metric)| OverlapView {
                        overlap_area: metric.overlap_area,
                        overlap_ratio: metric.overlap_ratio,
                        rule_id: ruleset
                            .string_id(*id)
                            .expect("rule id minted by this ruleset"),
                    })
                    .collect()
            }),
            rule_ids: rule_ids
                .iter()
                .map(|id| {
                    ruleset
                        .string_id(*id)
                        .expect("rule id minted by this ruleset")
                })
                .collect(),
        }),
    }
}

/// Assemble a whole batch of `CandidateOutcome`s (in input order) into the
/// JSON string a binding hands off — the wire contract (ADR-0004/0012/0018).
/// Every outcome streams straight into the serializer; no `serde_json::Value`
/// DOM is built (perf-memory 07). The payloads are built from domain types that
/// always serialize, so the call is infallible.
pub fn query_rich_json(ruleset: &Ruleset, outcomes: &[CandidateOutcome]) -> String {
    let views: Vec<OutcomeView> = outcomes
        .iter()
        .map(|outcome| candidate_outcome_view(ruleset, outcome))
        .collect();
    serde_json::to_string(&views).expect("candidate outcome payloads are always JSON-serializable")
}

/// Assemble a whole batch of `ResolutionOutcome`s (in input order) into the
/// JSON string a binding hands off — the wire contract (ADR-0015/0018).
/// Streams without a DOM, like [`query_rich_json`].
pub fn resolve_rich_json(ruleset: &Ruleset, outcomes: &[ResolutionOutcome]) -> String {
    let views: Vec<OutcomeView> = outcomes
        .iter()
        .map(|outcome| resolution_outcome_view(ruleset, outcome))
        .collect();
    serde_json::to_string(&views).expect("resolution outcome payloads are always JSON-serializable")
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

    /// Pins the exact wire bytes (perf-memory 07): the old implementation built a
    /// `serde_json::Map` (a `BTreeMap`, so alphabetical keys) and stringified it;
    /// the streaming serializer must emit the same bytes, key order included.
    #[test]
    fn query_rich_json_is_byte_identical_to_the_dom_shape() {
        let ruleset = ruleset();
        let candidates = candidates_from_geojson(CANDIDATES).unwrap();
        let query = Query::new(SpatialPredicate::Intersects);
        let outcomes = ruleset.query(&candidates, &query);

        assert_eq!(
            query_rich_json(&ruleset, &outcomes),
            r#"[{"outcome":"matched","ruleIds":["zone-a","zone-b"]},{"outcome":"notMatched"}]"#
        );
    }

    #[test]
    fn resolve_rich_json_is_byte_identical_to_the_dom_shape() {
        let ruleset = ruleset();
        let candidates = candidates_from_geojson(CANDIDATES).unwrap();
        let query = Query::new(SpatialPredicate::Intersects);
        let outcomes = ruleset.resolve(&candidates, &query);

        assert_eq!(
            resolve_rich_json(&ruleset, &outcomes),
            r#"[{"applicable":[{"priority":0,"propertyMatched":true,"ruleId":"zone-a","spatialMatched":true},{"priority":0,"propertyMatched":true,"ruleId":"zone-b","spatialMatched":true}],"outcome":"resolved","values":{"speedLimit":30},"winner":"zone-a"},{"outcome":"notMatched"}]"#
        );
    }
}