//! Python binding for `spatial-rules-core` (`.scratch/wasm`, ticket 02).
//!
//! The full Engine surface — query/resolve (mask `list[int]`), rich outcomes
//! (`list[dict]`), and the clock-backed `replace`/`to_canonical`/`stats` —
//! Pythonic in shape: rules/candidates/query accept `str | bytes | dict`, and
//! results come back as Python lists/dicts. JSON serialization is identical
//! to the napi/wasm paths (the same `Query` parse and the shared rich-JSON
//! serializers from `spatial-rules-bindings-common`). Errors raise
//! `SpatialRulesError` with an `SR_*` code in the message, matching the
//! wasm/napi contract.

use pyo3::create_exception;
use pyo3::exceptions::PyValueError;
use pyo3::prelude::*;

use spatial_rules_bindings_common::{
    parse_inputs, query_rich_json, report_to_json, resolve_rich_json, spatial_error_message,
};
use spatial_rules_core::{Engine, SpatialError};

create_exception!(spatial_rules, SpatialRulesError, PyValueError);

fn to_pyerr(error: SpatialError) -> PyErr {
    SpatialRulesError::new_err(spatial_error_message(&error))
}

/// The JSON string form of a Python input: `str` passthrough, `bytes`
/// UTF-8-decoded, anything else JSON-dumped by Python (dict/list/etc).
///
/// Failure to decode to a usable JSON string is reported as a structured
/// [`SpatialError`] with the caller-chosen `code`, so every input path raises
/// `SpatialRulesError` (an `SR_*` code) rather than a bare Python exception —
/// the documented contract at the top of this module.
fn any_to_json_string(
    obj: &Bound<'_, PyAny>,
    what: &str,
    code: spatial_rules_core::ErrorCode,
) -> Result<String, SpatialError> {
    if let Ok(text) = obj.extract::<String>() {
        return Ok(text);
    }
    if let Ok(bytes) = obj.extract::<Vec<u8>>() {
        return String::from_utf8(bytes).map_err(|e| {
            SpatialError::new(code, format!("{what} are not valid UTF-8: {e}"))
        });
    }
    let py = obj.py();
    let json = py
        .import("json")
        .map_err(|e| SpatialError::new(code, format!("{what} cannot encode as JSON: {e}")))?;
    let text = json
        .call_method1("dumps", (obj,))
        .and_then(|value| value.extract::<String>())
        .map_err(|e| SpatialError::new(code, format!("{what} cannot encode as JSON: {e}")))?;
    Ok(text)
}

fn parse_py_inputs(
    candidates: &Bound<'_, PyAny>,
    query: &Bound<'_, PyAny>,
) -> Result<(Vec<spatial_rules_core::Candidate>, spatial_rules_core::Query), SpatialError> {
    let candidates = any_to_json_string(
        candidates,
        "candidates",
        spatial_rules_core::ErrorCode::InvalidGeoJson,
    )?;
    let query = any_to_json_string(query, "query", spatial_rules_core::ErrorCode::InvalidQuery)?;
    parse_inputs(&candidates, &query)
}

/// A JSON string parsed by Python's `json.loads` into the corresponding
/// Python value (list/dict/scalar).
fn json_str_to_py(py: Python<'_>, json_str: &str) -> PyResult<PyObject> {
    let json = py.import("json")?;
    Ok(json.call_method1("loads", (json_str,))?.unbind())
}

/// A compiled ruleset with the full Engine surface (ADR-0007/0009/0015).
/// Construct via [`Ruleset::from_geojson`]; build once, then evaluate batches.
#[pyclass]
struct Ruleset {
    engine: Engine,
}

#[pymethods]
impl Ruleset {
    /// Compile a GeoJSON FeatureCollection of rules.
    #[staticmethod]
    fn from_geojson(rules: &Bound<'_, PyAny>) -> PyResult<Self> {
        let text = any_to_json_string(rules, "rules", spatial_rules_core::ErrorCode::InvalidGeoJson)
            .map_err(to_pyerr)?;
        let engine = Engine::from_geojson(&text).map_err(to_pyerr)?;
        Ok(Ruleset { engine })
    }

    /// Evaluate `query` against `candidates`; the compact mask as a list of
    /// ints: `0` no match, `1` matched, `2` invalid (ADR-0004).
    fn query(&self, candidates: &Bound<'_, PyAny>, query: &Bound<'_, PyAny>) -> PyResult<Vec<i64>> {
        let (candidates, query) = parse_py_inputs(candidates, query).map_err(to_pyerr)?;
        Ok(self.engine.query_mask(&candidates, &query).into_iter().map(i64::from).collect())
    }

    /// Resolve `query` against `candidates`; the compact mask as a list of
    /// ints: `0` no resolution, `1` resolved, `2` invalid (ADR-0015).
    fn resolve(&self, candidates: &Bound<'_, PyAny>, query: &Bound<'_, PyAny>) -> PyResult<Vec<i64>> {
        let (candidates, query) = parse_py_inputs(candidates, query).map_err(to_pyerr)?;
        Ok(self.engine.resolve_mask(&candidates, &query).into_iter().map(i64::from).collect())
    }

    /// Per-candidate match outcomes as a list of dicts (string rule ids,
    /// `includeOverlap`/`aggregate` payloads attached).
    fn query_rich(
        &self,
        py: Python<'_>,
        candidates: &Bound<'_, PyAny>,
        query: &Bound<'_, PyAny>,
    ) -> PyResult<PyObject> {
        let (candidates, query) = parse_py_inputs(candidates, query).map_err(to_pyerr)?;
        let ruleset = self.engine.snapshot();
        let outcomes = ruleset.query(&candidates, &query);
        let json_str = query_rich_json(&ruleset, &outcomes);
        json_str_to_py(py, &json_str)
    }

    /// Per-candidate resolution outcomes as a list of dicts
    /// (`{outcome, winner, values, applicable, aggregate}`, ADR-0015/0018).
    fn resolve_rich(
        &self,
        py: Python<'_>,
        candidates: &Bound<'_, PyAny>,
        query: &Bound<'_, PyAny>,
    ) -> PyResult<PyObject> {
        let (candidates, query) = parse_py_inputs(candidates, query).map_err(to_pyerr)?;
        let ruleset = self.engine.snapshot();
        let outcomes = ruleset.resolve(&candidates, &query);
        let json_str = resolve_rich_json(&ruleset, &outcomes);
        json_str_to_py(py, &json_str)
    }

    /// Atomically swap the ruleset from a GeoJSON FeatureCollection; returns
    /// the ADR-0007 observability report as a dict.
    fn replace(&self, rules: &Bound<'_, PyAny>) -> PyResult<PyObject> {
        let text = any_to_json_string(rules, "rules", spatial_rules_core::ErrorCode::InvalidGeoJson)
            .map_err(to_pyerr)?;
        let report = self.engine.replace_from_geojson(&text).map_err(to_pyerr)?;
        let json_str = serde_json::to_string(&report_to_json(report)).map_err(|e| {
            to_pyerr(SpatialError::new(
                spatial_rules_core::ErrorCode::Native,
                format!("serialize report: {e}"),
            ))
        })?;
        let py = rules.py();
        json_str_to_py(py, &json_str)
    }

    /// The validated rules as canonical JSON (a list of rule dicts, ADR-0013).
    fn to_canonical(&self, py: Python<'_>) -> PyResult<PyObject> {
        let bytes = self.engine.snapshot().to_canonical().map_err(to_pyerr)?;
        let json_str = String::from_utf8(bytes).map_err(|e| {
            to_pyerr(SpatialError::new(
                spatial_rules_core::ErrorCode::Native,
                format!("canonical ruleset is not valid UTF-8: {e}"),
            ))
        })?;
        json_str_to_py(py, &json_str)
    }

    /// Observability for the current ruleset as a dict.
    fn stats(&self, py: Python<'_>) -> PyResult<PyObject> {
        let json_str = serde_json::to_string(&report_to_json(self.engine.current())).map_err(|e| {
            to_pyerr(SpatialError::new(
                spatial_rules_core::ErrorCode::Native,
                format!("serialize report: {e}"),
            ))
        })?;
        json_str_to_py(py, &json_str)
    }
}

#[pymodule]
fn spatial_rules(m: &Bound<'_, PyModule>) -> PyResult<()> {
    m.add_class::<Ruleset>()?;
    m.add("SpatialRulesError", m.py().get_type::<SpatialRulesError>())?;
    m.add("__version__", env!("CARGO_PKG_VERSION"))?;
    Ok(())
}