//! Geometry validity gate (ADR-0005).
//!
//! Rule geometries are strict-rejected at ruleset build; candidate geometries
//! are checked at query time and reported as `invalid` per candidate rather
//! than failing the batch.

use geo::Geometry;
use geo::Validation;

use crate::error::SpatialError;

/// The geometry types supported in v1 (§2: Polygon and MultiPolygon).
pub fn ensure_supported_geometry(geometry: &Geometry<f64>) -> Result<(), SpatialError> {
    match geometry {
        Geometry::Polygon(_) | Geometry::MultiPolygon(_) => Ok(()),
        other => Err(SpatialError::unsupported_geometry_type(format!(
            "unsupported geometry type: {}",
            geometry_type_name(other)
        ))),
    }
}

/// Strict gate for rule geometries: supported type AND OGC-valid (ADR-0005).
pub fn validate_rule_geometry(geometry: &Geometry<f64>) -> Result<(), SpatialError> {
    ensure_supported_geometry(geometry)?;
    if !geometry.is_valid() {
        return Err(SpatialError::invalid_geometry(format!(
            "invalid rule geometry: {:?}",
            geometry.validation_errors()
        )));
    }
    Ok(())
}

fn geometry_type_name(geometry: &Geometry<f64>) -> &'static str {
    match geometry {
        Geometry::Point(_) => "Point",
        Geometry::Line(_) => "Line",
        Geometry::LineString(_) => "LineString",
        Geometry::Polygon(_) => "Polygon",
        Geometry::MultiPoint(_) => "MultiPoint",
        Geometry::MultiLineString(_) => "MultiLineString",
        Geometry::MultiPolygon(_) => "MultiPolygon",
        Geometry::GeometryCollection(_) => "GeometryCollection",
        Geometry::Rect(_) => "Rect",
        Geometry::Triangle(_) => "Triangle",
    }
}
