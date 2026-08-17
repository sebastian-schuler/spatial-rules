//! Node-API (napi-rs) binding for `spatial-rules-core` (ADR-0006).
//!
//! Hot path is byte-oriented: `query(buffer, query) -> Uint8Array` mask
//! (`0` = no match, `1` = matched, `2` = invalid). A richer API returns
//! per-candidate objects with original string rule ids. Construction/query
//! errors are thrown as JS errors carrying a stable `SR_*` code (ADR-0005).

use napi::bindgen_prelude::{Buffer, Uint8Array};
use napi::Error;
use napi_derive::napi;
use spatial_rules_core::{candidates_from_geojson, CandidateOutcome, Query, Ruleset, SpatialError};

fn spatial_error_to_napi(error: SpatialError) -> Error<&'static str> {
    Error::new(error.code.as_str(), error.message)
}

fn bytes_to_str<'a>(buffer: &'a Buffer, kind: &str) -> napi::Result<&'a str, &'static str> {
    std::str::from_utf8(buffer.as_ref()).map_err(|e| {
        Error::new(
            "SR_INVALID_GEOJSON",
            format!("{kind} are not valid UTF-8: {e}"),
        )
    })
}

fn parse_query(query_json: &str) -> napi::Result<Query, &'static str> {
    let value: serde_json::Value = serde_json::from_str(query_json)
        .map_err(|e| Error::new("SR_INVALID_QUERY", format!("query is not valid JSON: {e}")))?;
    Query::from_json(&value).map_err(spatial_error_to_napi)
}

#[napi]
pub struct SpatialRuleset {
    ruleset: Ruleset,
}

#[napi]
impl SpatialRuleset {
    /// Construct an immutable ruleset from a GeoJSON FeatureCollection `Buffer`.
    #[napi(constructor)]
    pub fn new(rules: Buffer) -> napi::Result<Self, &'static str> {
        let text = bytes_to_str(&rules, "rules")?;
        let ruleset = Ruleset::from_geojson(text).map_err(spatial_error_to_napi)?;
        Ok(SpatialRuleset { ruleset })
    }

    /// Evaluate candidates (GeoJSON `Buffer`) against `query` (JSON string) and
    /// return a `Uint8Array` mask: `0` no match, `1` matched, `2` invalid.
    #[napi]
    pub fn query(&self, candidates: Buffer, query: String) -> napi::Result<Uint8Array, &'static str> {
        let text = bytes_to_str(&candidates, "candidates")?;
        let candidates = candidates_from_geojson(text).map_err(spatial_error_to_napi)?;
        let query = parse_query(&query)?;
        let outcomes = self.ruleset.query(&candidates, &query);
        let mut mask = vec![0u8; outcomes.len()];
        for (index, outcome) in outcomes.iter().enumerate() {
            mask[index] = match outcome {
                CandidateOutcome::NotMatched => 0,
                CandidateOutcome::Matched { .. } => 1,
                CandidateOutcome::Invalid { .. } => 2,
            };
        }
        Ok(Uint8Array::from(mask))
    }

    /// Rich per-candidate outcomes as a JSON string (string rule ids, invalid
    /// reasons), aligned to input order (ADR-0004).
    #[napi]
    pub fn query_rich(&self, candidates: Buffer, query: String) -> napi::Result<String, &'static str> {
        let text = bytes_to_str(&candidates, "candidates")?;
        let candidates = candidates_from_geojson(text).map_err(spatial_error_to_napi)?;
        let query = parse_query(&query)?;
        let outcomes = self.ruleset.query(&candidates, &query);
        let rich: Vec<serde_json::Value> = outcomes
            .iter()
            .map(|outcome| match outcome {
                CandidateOutcome::NotMatched => serde_json::json!({ "outcome": "notMatched" }),
                CandidateOutcome::Matched { rule_ids } => {
                    let ids: Vec<&str> = rule_ids
                        .iter()
                        .map(|id| self.ruleset.string_id(*id))
                        .collect();
                    serde_json::json!({ "outcome": "matched", "ruleIds": ids })
                }
                CandidateOutcome::Invalid { reason } => {
                    serde_json::json!({ "outcome": "invalid", "reason": reason })
                }
            })
            .collect();
        serde_json::to_string(&rich)
            .map_err(|e| Error::new("SR_NATIVE", format!("serialize result: {e}")))
    }
}
