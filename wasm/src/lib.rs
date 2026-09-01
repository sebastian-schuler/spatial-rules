//! WASM binding for `spatial-rules-core` (`.scratch/wasm`, ticket 01).
//!
//! **Ruleset-level surface only**: build once, then `query`/`resolve` (mask +
//! rich JSON) and `toCanonical`. No `Engine` `replace`/`stats` (their
//! clock-based observability is degenerate on wasm — there is no clock), no
//! async (the engine is sync and whole-buffer). Errors are thrown as JS
//! `Error`s whose message carries the stable `SR_*` code (`"SR_CODE: message"`),
//! reconstructed by the TS wrapper's `SpatialRulesError` — the same contract
//! the napi async path uses.
//!
//! The rich-JSON serializers are shared by all three bindings (node, wasm,
//! python) via `spatial-rules-bindings-common` — identical payload shapes.

use spatial_rules_bindings_common::{
    parse_query, query_rich_json, resolve_rich_json, spatial_error_message,
};
use spatial_rules_core::{candidates_from_geojson, Candidate, ErrorCode, Query, Ruleset, SpatialError};
use wasm_bindgen::prelude::*;

fn spatial_error_to_js(error: SpatialError) -> JsError {
    JsError::new(&spatial_error_message(&error))
}

fn parse_inputs(candidates: &str, query: &str) -> Result<(Vec<Candidate>, Query), SpatialError> {
    let candidates = candidates_from_geojson(candidates)?;
    let query = parse_query(query)?;
    Ok((candidates, query))
}

/// A compiled ruleset ready to evaluate candidate batches. Mirrors the Node
/// wrapper's `SpatialRuleset` minus the Engine-level `replace`/`stats` and the
/// async paths.
#[wasm_bindgen]
pub struct SpatialRuleset {
    ruleset: Ruleset,
}

#[wasm_bindgen]
impl SpatialRuleset {
    /// Compile a GeoJSON FeatureCollection of rules.
    #[wasm_bindgen(constructor)]
    pub fn new(rules: &str) -> Result<SpatialRuleset, JsError> {
        let ruleset = Ruleset::from_geojson(rules).map_err(spatial_error_to_js)?;
        Ok(SpatialRuleset { ruleset })
    }

    /// Evaluate `query` against `candidates` (GeoJSON strings) and return the
    /// compact mask: `0` no match, `1` matched, `2` invalid (ADR-0004).
    pub fn query(&self, candidates: &str, query: &str) -> Result<Vec<u8>, JsError> {
        let (candidates, query) = parse_inputs(candidates, query).map_err(spatial_error_to_js)?;
        Ok(self.ruleset.query_mask(&candidates, &query))
    }

    /// Resolve `query` against `candidates` and return the compact mask:
    /// `0` no resolution, `1` resolved, `2` invalid (ADR-0015).
    pub fn resolve(&self, candidates: &str, query: &str) -> Result<Vec<u8>, JsError> {
        let (candidates, query) = parse_inputs(candidates, query).map_err(spatial_error_to_js)?;
        Ok(self.ruleset.resolve_mask(&candidates, &query))
    }

    /// Per-candidate match outcomes as a JSON string (string rule ids,
    /// `includeOverlap`/`aggregate` payloads attached).
    pub fn query_rich(&self, candidates: &str, query: &str) -> Result<String, JsError> {
        let (candidates, query) = parse_inputs(candidates, query).map_err(spatial_error_to_js)?;
        let outcomes = self.ruleset.query(&candidates, &query);
        Ok(query_rich_json(&self.ruleset, &outcomes))
    }

    /// Per-candidate resolution outcomes as a JSON string
    /// (`{outcome, winner, values, applicable, aggregate}`, ADR-0015/0018).
    pub fn resolve_rich(&self, candidates: &str, query: &str) -> Result<String, JsError> {
        let (candidates, query) = parse_inputs(candidates, query).map_err(spatial_error_to_js)?;
        let outcomes = self.ruleset.resolve(&candidates, &query);
        Ok(resolve_rich_json(&self.ruleset, &outcomes))
    }

    /// The validated rules as canonical JSON (ADR-0013).
    pub fn to_canonical(&self) -> Result<String, JsError> {
        let bytes = self.ruleset.to_canonical().map_err(spatial_error_to_js)?;
        String::from_utf8(bytes).map_err(|e| {
            spatial_error_to_js(SpatialError::new(
                ErrorCode::Native,
                format!("canonical ruleset is not valid UTF-8: {e}"),
            ))
        })
    }
}