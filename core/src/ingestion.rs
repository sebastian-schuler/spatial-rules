//! GeoJSON → `geo_types::Geometry<f64>` ingestion.
//!
//! Parsing is permissive on geometry; validity is a separate gate — see
//! [`validate_rule_geometry`](crate::validation::validate_rule_geometry).

use geo::Geometry;

use crate::candidate::Candidate;
use crate::error::SpatialError;
use crate::properties::properties_from_json;
use crate::rule::Rule;

/// Parse a GeoJSON document (permissive geometry; strict on malformed JSON).
pub fn parse_geojson(input: &str) -> Result<geojson::GeoJson, SpatialError> {
    input
        .parse::<geojson::GeoJson>()
        .map_err(|e| SpatialError::invalid_geojson(format!("failed to parse GeoJSON: {e}")))
}

/// Extract the geometry of a feature as `geo::Geometry<f64>`.
///
/// The `geojson` crate's parse is permissive; geo validity is checked
/// separately by [`validate_rule_geometry`](crate::validation::validate_rule_geometry).
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
    Ok(Rule {
        id,
        properties,
        geometry,
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

/// Parse a GeoJSON FeatureCollection into candidates.
pub fn candidates_from_geojson(input: &str) -> Result<Vec<Candidate>, SpatialError> {
    let features = features_of(input)?;
    features.iter().map(candidate_from_feature).collect()
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
