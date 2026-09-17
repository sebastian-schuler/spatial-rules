//! GeoJSON → `geo_types::Geometry<f64>` ingestion.
//!
//! Parsing is permissive on geometry; validity is a separate gate — see
//! [`validate_rule_geometry`](crate::model::validation::validate_rule_geometry).

use geo::Geometry;
use serde::Deserialize;

use crate::model::candidate::Candidate;
use crate::error::{ErrorCode, SpatialError};
use crate::model::properties::properties_from_json;
use crate::model::rule::Rule;

/// Parse a GeoJSON document (permissive geometry; strict on malformed JSON).
pub fn parse_geojson(input: &str) -> Result<geojson::GeoJson, SpatialError> {
    input
        .parse::<geojson::GeoJson>()
        .map_err(|e| SpatialError::invalid_geojson(format!("failed to parse GeoJSON: {e}")))
}

/// Extract the geometry of a feature as `geo::Geometry<f64>`.
///
/// The `geojson` crate's parse is permissive; geo validity is checked
/// separately by [`validate_rule_geometry`](crate::model::validation::validate_rule_geometry).
pub fn feature_geometry(feature: &geojson::Feature) -> Result<Geometry<f64>, SpatialError> {
    let geometry = feature
        .geometry
        .as_ref()
        .ok_or_else(|| SpatialError::invalid_geojson("feature is missing a geometry"))?;
    Geometry::try_from(geometry).map_err(|e: geojson::Error| {
        SpatialError::invalid_geojson(format!("unsupported or malformed geometry: {e}"))
    })
}

/// Build a [`Rule`] from a GeoJSON feature.
pub fn rule_from_feature(feature: &geojson::Feature) -> Result<Rule, SpatialError> {
    let id = extract_feature_id(feature)?;
    let geometry = feature_geometry(feature)?;
    let properties = feature
        .properties
        .as_ref()
        .map(properties_from_json)
        .unwrap_or_default();
    let priority = extract_feature_priority(feature, &id)?;
    Ok(Rule {
        id,
        properties,
        geometry,
        priority,
    })
}

/// Build a [`Candidate`] from a GeoJSON feature, classifying it at intake
/// (architecture-hardening 01): the candidate carries its envelope (valid) or
/// invalid reason, so the query hot path never re-derives it.
pub fn candidate_from_feature(feature: &geojson::Feature) -> Result<Candidate, SpatialError> {
    let id = extract_feature_id(feature)?;
    let geometry = feature_geometry(feature)?;
    Ok(Candidate::new(id, geometry))
}

/// Parse a GeoJSON FeatureCollection into rules.
pub fn rules_from_geojson(input: &str) -> Result<Vec<Rule>, SpatialError> {
    let features = features_of(input)?;
    features.iter().map(rule_from_feature).collect()
}

/// Parse a GeoJSON FeatureCollection (or a single Feature) into candidates.
///
/// Uses a candidate-specific deserializer rather than
/// [`parse_geojson`]/[`candidate_from_feature`], because candidate **properties
/// are discarded** — `Candidate` carries only an id and a geometry. The
/// `geojson` crate would still parse every property into a
/// `Map<String, Value>`; on a realistic payload that costs more than parsing the
/// coordinates (measured ~1.24 ms of a ~5.8 ms engine total at 1,000 candidates
/// with 8 properties each, versus ~1.09 ms for the empty-properties harness
/// data). This path reads the top-level `id`, the geometry, and — only for the
/// id fallback — `properties.id`, streaming past every other property without
/// allocating. Rules keep the `geojson` path: their properties are queryable.
///
/// Errors match the `geojson` path's shape and code (`SR_INVALID_GEOJSON`):
/// malformed JSON, an unexpected `type`, a missing id, and a missing or
/// malformed geometry all surface as [`ErrorCode::InvalidGeoJson`].
pub fn candidates_from_geojson(input: &str) -> Result<Vec<Candidate>, SpatialError> {
    let document: RawCandidateDocument = serde_json::from_str(input).map_err(|e| {
        SpatialError::invalid_geojson(format!("failed to parse GeoJSON: {e}"))
    })?;
    let RawCandidateDocument {
        kind,
        features,
        id,
        geometry,
        properties,
    } = document;

    match kind.as_deref() {
        Some("FeatureCollection") => {
            let features = features.ok_or_else(|| {
                SpatialError::invalid_geojson(
                    "failed to parse GeoJSON: missing field `features`",
                )
            })?;
            features.iter().map(candidate_from_raw_feature).collect()
        }
        Some("Feature") => {
            let feature = RawCandidateFeature {
                id,
                geometry,
                properties,
            };
            Ok(vec![candidate_from_raw_feature(&feature)?])
        }
        Some(other) => Err(SpatialError::invalid_geojson(format!(
            "expected a FeatureCollection or Feature, found type `{other}`"
        ))),
        None => Err(SpatialError::invalid_geojson(
            "failed to parse GeoJSON: missing field `type`",
        )),
    }
}

/// A GeoJSON document with only the members the candidate path reads. Unknown
/// members are skipped by serde without building a value for them.
#[derive(Deserialize)]
struct RawCandidateDocument {
    #[serde(rename = "type", default)]
    kind: Option<String>,
    #[serde(default)]
    features: Option<Vec<RawCandidateFeature>>,
    #[serde(default)]
    id: Option<geojson::feature::Id>,
    #[serde(default)]
    geometry: Option<geojson::Geometry>,
    #[serde(default)]
    properties: Option<CandidateProperties>,
}

#[derive(Deserialize)]
struct RawCandidateFeature {
    #[serde(default)]
    id: Option<geojson::feature::Id>,
    #[serde(default)]
    geometry: Option<geojson::Geometry>,
    #[serde(default)]
    properties: Option<CandidateProperties>,
}

/// The only candidate property the engine ever reads: the `properties.id`
/// fallback for the feature id. Everything else is skipped.
#[derive(Deserialize)]
struct CandidateProperties {
    #[serde(default)]
    id: Option<serde_json::Value>,
}

fn candidate_from_raw_feature(feature: &RawCandidateFeature) -> Result<Candidate, SpatialError> {
    let id = raw_feature_id(feature)?;
    let geometry = raw_feature_geometry(feature)?;
    Ok(Candidate::new(id, geometry))
}

fn raw_feature_id(feature: &RawCandidateFeature) -> Result<String, SpatialError> {
    if let Some(id) = &feature.id {
        return Ok(id_to_string(id));
    }
    if let Some(properties) = &feature.properties {
        if let Some(serde_json::Value::String(s)) = &properties.id {
            return Ok(s.clone());
        }
    }
    Err(SpatialError::invalid_geojson(
        "feature is missing an `id` (set `id` or `properties.id`)",
    ))
}

fn raw_feature_geometry(feature: &RawCandidateFeature) -> Result<Geometry<f64>, SpatialError> {
    let geometry = feature
        .geometry
        .as_ref()
        .ok_or_else(|| SpatialError::invalid_geojson("feature is missing a geometry"))?;
    Geometry::try_from(geometry).map_err(|e: geojson::Error| {
        SpatialError::invalid_geojson(format!("unsupported or malformed geometry: {e}"))
    })
}

fn features_of(input: &str) -> Result<Vec<geojson::Feature>, SpatialError> {
    match parse_geojson(input)? {
        geojson::GeoJson::FeatureCollection(collection) => Ok(collection.features),
        geojson::GeoJson::Feature(feature) => Ok(vec![feature]),
        _ => Err(SpatialError::invalid_geojson(
            "expected a FeatureCollection or Feature",
        )),
    }
}

fn extract_feature_id(feature: &geojson::Feature) -> Result<String, SpatialError> {
    if let Some(id) = &feature.id {
        return Ok(id_to_string(id));
    }
    if let Some(properties) = &feature.properties {
        if let Some(serde_json::Value::String(s)) = properties.get("id") {
            return Ok(s.clone());
        }
    }
    Err(SpatialError::invalid_geojson(
        "feature is missing an `id` (set `id` or `properties.id`)",
    ))
}

fn id_to_string(id: &geojson::feature::Id) -> String {
    match id {
        geojson::feature::Id::String(s) => s.clone(),
        geojson::feature::Id::Number(n) => n.to_string(),
    }
}

/// Read the top-level `priority` foreign member (ADR-0015). Missing → `0`.
/// A present value that is not an integer (string/float/bool) → a construction
/// error naming the rule. (Non-negativity is enforced at compile —
/// [`Ruleset::build_with`](crate::runtime::ruleset::Ruleset::build_with) — the single
/// authoritative gate every construction path passes through.)
fn extract_feature_priority(
    feature: &geojson::Feature,
    rule_id: &str,
) -> Result<i64, SpatialError> {
    let Some(foreign_members) = &feature.foreign_members else {
        return Ok(0);
    };
    let Some(priority) = foreign_members.get("priority") else {
        return Ok(0);
    };
    validate_priority(rule_id, priority)
}

/// Validate that a present top-level `priority` is an integer (ADR-0015),
/// so it can become the `Rule.priority` field. Shared by the GeoJSON
/// ingestion gate and the canonical load gate so both fail with the same
/// `SR_RULESET_CONSTRUCTION_FAILED` code and name the rule. This is the
/// **typedness** gate only — non-negativity is enforced once, at compile
/// (`Ruleset::build_with`), which every construction path passes through.
pub(crate) fn validate_priority(
    rule_id: &str,
    found: &serde_json::Value,
) -> Result<i64, SpatialError> {
    found.as_i64().ok_or_else(|| {
        SpatialError::new(
            ErrorCode::RulesetConstructionFailed,
            format!("rule '{rule_id}': top-level 'priority' must be an integer, found {found}"),
        )
    })
}
