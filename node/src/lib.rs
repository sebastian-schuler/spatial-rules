//! Node-API (napi-rs) binding for `spatial-rules-core` (ADR-0006).
//!
//! Hot path is byte-oriented: `query(buffer, query) -> Uint8Array` mask
//! (`0` = no match, `1` = matched, `2` = invalid). A richer API returns
//! per-candidate objects with original string rule ids. `replace(buffer)`
//! swaps the active ruleset atomically and returns ADR-0007 observability.
//! Construction/query errors are thrown as JS errors carrying a stable `SR_*`
//! code (ADR-0005).

use napi::bindgen_prelude::{Buffer, Uint8Array};
use napi::Error;
use napi_derive::napi;
use spatial_rules_core::{
    candidates_from_geojson, Candidate, CandidateOutcome, Engine, ErrorCode, Query, ReplaceReport,
    SpatialError,
};

fn spatial_error_to_napi(error: SpatialError) -> Error<&'static str> {
    Error::new(error.code.as_str(), error.message)
}

fn bytes_to_str<'a>(buffer: &'a Buffer, kind: &str) -> Result<&'a str, SpatialError> {
    std::str::from_utf8(buffer.as_ref())
        .map_err(|e| SpatialError::invalid_geojson(format!("{kind} are not valid UTF-8: {e}")))
}

fn parse_query(query_json: &str) -> Result<Query, SpatialError> {
    let value: serde_json::Value = serde_json::from_str(query_json)
        .map_err(|e| SpatialError::invalid_query(format!("query is not valid JSON: {e}")))?;
    Query::from_json(&value)
}

/// Parse a candidates buffer and query string into the types the engine needs.
fn parse_inputs(
    candidates: Buffer,
    query: String,
) -> napi::Result<(Vec<Candidate>, Query), &'static str> {
    let text = bytes_to_str(&candidates, "candidates").map_err(spatial_error_to_napi)?;
    let candidates = candidates_from_geojson(text).map_err(spatial_error_to_napi)?;
    let query = parse_query(&query).map_err(spatial_error_to_napi)?;
    Ok((candidates, query))
}

fn report_to_json(report: ReplaceReport) -> serde_json::Value {
    serde_json::json!({
        "version": report.version,
        "ruleCount": report.rule_count,
        "buildDurationMs": report.build_duration_ms,
        "lastSwapTime": report.last_swap_time_unix_ms,
    })
}

fn report_to_string(report: ReplaceReport) -> napi::Result<String, &'static str> {
    serde_json::to_string(&report_to_json(report)).map_err(|e| {
        spatial_error_to_napi(SpatialError::new(
            ErrorCode::Native,
            format!("serialize report: {e}"),
        ))
    })
}

#[napi]
pub struct SpatialRuleset {
    engine: Engine,
}

#[napi]
impl SpatialRuleset {
    /// Construct an engine from a GeoJSON FeatureCollection `Buffer`.
    #[napi(constructor)]
    pub fn new(rules: Buffer) -> napi::Result<Self, &'static str> {
        let text = bytes_to_str(&rules, "rules").map_err(spatial_error_to_napi)?;
        let engine = Engine::from_geojson(text).map_err(spatial_error_to_napi)?;
        Ok(SpatialRuleset { engine })
    }

    /// Evaluate candidates (GeoJSON `Buffer`) against `query` (JSON string) and
    /// return a `Uint8Array` mask: `0` no match, `1` matched, `2` invalid.
    #[napi]
    pub fn query(&self, candidates: Buffer, query: String) -> napi::Result<Uint8Array, &'static str> {
        let (candidates, query) = parse_inputs(candidates, query)?;
        Ok(Uint8Array::from(self.engine.query_mask(&candidates, &query)))
    }

    /// Rich per-candidate outcomes as a JSON string (string rule ids, invalid
    /// reasons), aligned to input order (ADR-0004). Honors `includeOverlap`
    /// (ADR-0012): when set, each matched candidate also carries per-rule
    /// `overlapArea`/`overlapRatio` geodesic metrics.
    #[napi]
    pub fn query_rich(&self, candidates: Buffer, query: String) -> napi::Result<String, &'static str> {
        let (candidates, query) = parse_inputs(candidates, query)?;
        // Snapshot once so outcomes and their string ids come from the same
        // ruleset (a concurrent replace can't tear them apart, ADR-0007).
        let ruleset = self.engine.snapshot();
        let outcomes = ruleset.query(&candidates, &query);
        let rich: Vec<serde_json::Value> = outcomes
            .iter()
            .map(|outcome| match outcome {
                CandidateOutcome::NotMatched => serde_json::json!({ "outcome": "notMatched" }),
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
                    serde_json::Value::Object(object)
                }
                CandidateOutcome::Invalid { reason } => {
                    serde_json::json!({ "outcome": "invalid", "reason": reason })
                }
            })
            .collect();
        serde_json::to_string(&rich).map_err(|e| {
            spatial_error_to_napi(SpatialError::new(
                ErrorCode::Native,
                format!("serialize result: {e}"),
            ))
        })
    }

    /// Replace the active ruleset from a GeoJSON FeatureCollection `Buffer`,
    /// fully built off the hot path and published atomically. Returns ADR-0007
    /// observability as a JSON string.
    #[napi]
    pub fn replace(&self, rules: Buffer) -> napi::Result<String, &'static str> {
        let text = bytes_to_str(&rules, "rules").map_err(spatial_error_to_napi)?;
        let report = self
            .engine
            .replace_from_geojson(text)
            .map_err(spatial_error_to_napi)?;
        report_to_string(report)
    }

    /// Observability for the current ruleset as a JSON string.
    #[napi]
    pub fn stats(&self) -> napi::Result<String, &'static str> {
        report_to_string(self.engine.current())
    }
}
