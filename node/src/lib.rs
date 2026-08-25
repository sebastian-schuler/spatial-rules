//! Node-API (napi-rs) binding for `spatial-rules-core` (ADR-0006).
//!
//! Hot path is byte-oriented: `query(buffer, query) -> Uint8Array` mask
//! (`0` = no match, `1` = matched, `2` = invalid). A richer API returns
//! per-candidate objects with original string rule ids. `replace(buffer)`
//! swaps the active ruleset atomically and returns ADR-0007 observability.
//! Construction/query errors are thrown as JS errors carrying a stable `SR_*`
//! code (ADR-0005).

use std::sync::Arc;

use napi::bindgen_prelude::{Buffer, Uint8Array};
use napi::Error;
use napi_derive::napi;
use spatial_rules_core::{
    candidates_from_geojson, AggregateSpec, Candidate, CandidateOutcome, Engine, ErrorCode, Query,
    ReplaceReport, ResolutionOutcome, RuleId, Ruleset, SpatialError,
};

fn spatial_error_to_napi(error: SpatialError) -> Error<&'static str> {
    Error::new(error.code.as_str(), error.message)
}

/// Async rejections surface as the default `napi::Error` (a `Status` enum code),
/// which cannot carry a custom `SR_*` code in `.code`. Embed the code in the
/// message instead; the JS wrapper reconstructs `SpatialRulesError` from it
/// (ADR-0005).
fn spatial_error_to_napi_async(error: SpatialError) -> Error {
    Error::from_reason(format!("{}: {}", error.code, error.message))
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

/// Parse candidate bytes + query JSON into the engine types. The caller maps
/// the `SpatialError` to the sync (`Error<&'static str>`) or async (`Error`)
/// napi error shape.
fn parse_inputs_core(
    candidates: &[u8],
    query: &str,
) -> Result<(Vec<Candidate>, Query), SpatialError> {
    let text = std::str::from_utf8(candidates)
        .map_err(|e| SpatialError::invalid_geojson(format!("candidates are not valid UTF-8: {e}")))?;
    let candidates = candidates_from_geojson(text)?;
    let query = parse_query(query)?;
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

/// The requested per-candidate aggregate as the ADR-0018 JSON object — only
/// the functions the spec asked for (and that produced a value) are emitted.
fn aggregate_json(
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
/// strings (mapped via the snapshot, so a concurrent replace can't tear them
/// apart). `values` uses the rules' compact typed properties, which serialize
/// to the plain JSON scalars they wrap (ADR-0013).
fn resolution_outcome_to_json(
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

#[napi]
#[derive(Clone)]
pub struct SpatialRuleset {
    engine: Arc<Engine>,
}

#[napi]
impl SpatialRuleset {
    /// Construct an engine from a GeoJSON FeatureCollection `Buffer`.
    #[napi(constructor)]
    pub fn new(rules: Buffer) -> napi::Result<Self, &'static str> {
        let text = bytes_to_str(&rules, "rules").map_err(spatial_error_to_napi)?;
        let engine = Engine::from_geojson(text).map_err(spatial_error_to_napi)?;
        Ok(SpatialRuleset {
            engine: Arc::new(engine),
        })
    }

    /// Evaluate candidates (GeoJSON `Buffer`) against `query` (JSON string) and
    /// return a `Uint8Array` mask: `0` no match, `1` matched, `2` invalid.
    #[napi]
    pub fn query(&self, candidates: Buffer, query: String) -> napi::Result<Uint8Array, &'static str> {
        let (candidates, query) =
            parse_inputs_core(candidates.as_ref(), &query).map_err(spatial_error_to_napi)?;
        Ok(Uint8Array::from(self.engine.query_mask(&candidates, &query)))
    }

    /// Opt-in off-main-thread query (ADR-0009 amendment): returns the same
    /// mask as [`SpatialRuleset::query`], but the parse + query run on libuv's
    /// threadpool so the JS event loop stays responsive. The candidate
    /// `Buffer` is copied once per call (buffers are not moved across
    /// threads). Errors reject with the same `SR_*` code/message as the sync
    /// path.
    #[napi]
    pub async fn query_async(
        &self,
        candidates: Buffer,
        query: String,
    ) -> napi::Result<Uint8Array> {
        let candidates_bytes = candidates.as_ref().to_vec();
        let (candidates, query) =
            parse_inputs_core(&candidates_bytes, &query).map_err(spatial_error_to_napi_async)?;
        Ok(Uint8Array::from(self.engine.query_mask(&candidates, &query)))
    }

    /// Resolve candidates against `query` and return a `Uint8Array` mask:
    /// `0` no resolution, `1` resolved, `2` invalid (ADR-0015). The hot mask
    /// is computed without materialising the winner, values, or explanation —
    /// `resolveRich` is the opt-in rich call.
    #[napi]
    pub fn resolve(
        &self,
        candidates: Buffer,
        query: String,
    ) -> napi::Result<Uint8Array, &'static str> {
        let (candidates, query) =
            parse_inputs_core(candidates.as_ref(), &query).map_err(spatial_error_to_napi)?;
        Ok(Uint8Array::from(self.engine.resolve_mask(&candidates, &query)))
    }

    /// Opt-in off-main-thread resolution (ADR-0009 mirror): same mask as
    /// [`SpatialRuleset::resolve`], computed on libuv's threadpool.
    #[napi]
    pub async fn resolve_async(
        &self,
        candidates: Buffer,
        query: String,
    ) -> napi::Result<Uint8Array> {
        let candidates_bytes = candidates.as_ref().to_vec();
        let (candidates, query) =
            parse_inputs_core(&candidates_bytes, &query).map_err(spatial_error_to_napi_async)?;
        Ok(Uint8Array::from(self.engine.resolve_mask(&candidates, &query)))
    }

    /// Rich per-candidate resolution outcomes as a JSON string (string winner
    /// and rule ids, merged values, and the ordered applicable set with its
    /// `spatialMatched`/`propertyMatched` flags), aligned to input order
    /// (ADR-0015). The one lazy rich call the wrapper defers until asked.
    #[napi]
    pub fn resolve_rich(
        &self,
        candidates: Buffer,
        query: String,
    ) -> napi::Result<String, &'static str> {
        let (candidates, query) =
            parse_inputs_core(candidates.as_ref(), &query).map_err(spatial_error_to_napi)?;
        // Snapshot once so outcomes and their string ids come from the same
        // ruleset (a concurrent replace can't tear them apart, ADR-0007).
        let ruleset = self.engine.snapshot();
        let outcomes = ruleset.resolve(&candidates, &query);
        let rich: Vec<serde_json::Value> = outcomes
            .iter()
            .zip(candidates.iter())
            .map(|(outcome, candidate)| {
                resolution_outcome_to_json(&ruleset, candidate, query.aggregate.as_ref(), outcome)
            })
            .collect();
        serde_json::to_string(&rich).map_err(|e| {
            spatial_error_to_napi(SpatialError::new(
                ErrorCode::Native,
                format!("serialize result: {e}"),
            ))
        })
    }

    /// Rich per-candidate outcomes as a JSON string (string rule ids, invalid
    /// reasons), aligned to input order (ADR-0004). Honors `includeOverlap`
    /// (ADR-0012): when set, each matched candidate also carries per-rule
    /// `overlapArea`/`overlapRatio` geodesic metrics.
    #[napi]
    pub fn query_rich(&self, candidates: Buffer, query: String) -> napi::Result<String, &'static str> {
        let (candidates, query) =
            parse_inputs_core(candidates.as_ref(), &query).map_err(spatial_error_to_napi)?;
        // Snapshot once so outcomes and their string ids come from the same
        // ruleset (a concurrent replace can't tear them apart, ADR-0007).
        let ruleset = self.engine.snapshot();
        let outcomes = ruleset.query(&candidates, &query);
        let rich: Vec<serde_json::Value> = outcomes
            .iter()
            .zip(candidates.iter())
            .map(|(outcome, candidate)| match outcome {
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
                    if let Some(spec) = &query.aggregate {
                        object.insert(
                            "aggregate".to_string(),
                            aggregate_json(spec, candidate, rule_ids, &ruleset),
                        );
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

    /// Serialize the current ruleset to its canonical JSON form (ADR-0013):
    /// the validated rules, not the compiled indexes.
    #[napi(js_name = "toJSON")]
    pub fn to_json(&self) -> napi::Result<String, &'static str> {
        let ruleset = self.engine.snapshot();
        let bytes = ruleset.to_canonical().map_err(spatial_error_to_napi)?;
        String::from_utf8(bytes).map_err(|e| {
            spatial_error_to_napi(SpatialError::new(
                ErrorCode::Native,
                format!("canonical ruleset is not valid UTF-8: {e}"),
            ))
        })
    }

    /// Replace the active ruleset from canonical JSON `Buffer`, built off the
    /// hot path and published atomically (ADR-0013). A failed load keeps the
    /// old ruleset. Returns ADR-0007 observability as a JSON string.
    #[napi]
    pub fn from_canonical(&self, rules: Buffer) -> napi::Result<String, &'static str> {
        let report = self
            .engine
            .replace_from_canonical(rules.as_ref())
            .map_err(spatial_error_to_napi)?;
        report_to_string(report)
    }
}
