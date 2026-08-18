//! Geometry validity gate (ADR-0005).
//!
//! Rule geometries are strict-rejected at ruleset build; candidate geometries
//! are checked at query time and reported as `invalid` per candidate rather
//! than failing the batch.

use geo::{BoundingRect, Geometry, Rect};
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

/// Classify a candidate geometry for the query pipeline (ADR-0005): it must use
/// a supported type AND be OGC-valid AND have a bounding rectangle. Success
/// returns the precomputed envelope the spatial step needs; failure returns the
/// human-readable `Invalid` reason.
pub fn classify_candidate(geometry: &Geometry<f64>) -> Result<Rect<f64>, String> {
    ensure_supported_geometry(geometry).map_err(|error| error.message)?;
    if !geometry.is_valid() {
        return Err(format!(
            "invalid geometry: {:?}",
            geometry.validation_errors()
        ));
    }
    geometry
        .bounding_rect()
        .ok_or_else(|| "geometry has no bounding rectangle".to_string())
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

#[cfg(test)]
mod tests {
    use super::*;
    use geo::{LineString, Point, Polygon};

    fn square() -> Polygon<f64> {
        Polygon::new(
            LineString::from(vec![
                (0.0, 0.0),
                (0.0, 10.0),
                (10.0, 10.0),
                (10.0, 0.0),
                (0.0, 0.0),
            ]),
            vec![],
        )
    }

    #[test]
    fn valid_polygon_returns_its_envelope() {
        let rect = classify_candidate(&Geometry::Polygon(square())).unwrap();
        assert_eq!(rect, Rect::new((0.0, 0.0), (10.0, 10.0)));
    }

    #[test]
    fn unsupported_type_is_rejected() {
        let reason = classify_candidate(&Geometry::Point(Point::new(1.0, 1.0))).unwrap_err();
        assert_eq!(reason, "unsupported geometry type: Point");
    }

    #[test]
    fn invalid_geometry_is_rejected() {
        let bowtie = Polygon::new(
            LineString::from(vec![
                (0.0, 0.0),
                (10.0, 10.0),
                (0.0, 10.0),
                (10.0, 0.0),
                (0.0, 0.0),
            ]),
            vec![],
        );
        let reason = classify_candidate(&Geometry::Polygon(bowtie)).unwrap_err();
        assert!(reason.starts_with("invalid geometry:"));
    }
}
