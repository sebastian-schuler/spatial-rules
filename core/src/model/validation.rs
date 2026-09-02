//! Geometry validity gate (ADR-0005).
//!
//! Rule geometries are strict-rejected at ruleset build; candidate geometries
//! are checked at query time and reported as `invalid` per candidate rather
//! than failing the batch.

use geo::{BoundingRect, Geometry, Rect};
use geo::Validation;

use crate::error::SpatialError;

/// Whether every coordinate in `geometry` is finite. Non-finite (`NaN`/`±∞`)
/// coordinates would make geo's `Relate`/`Validation` paths panic (they `unwrap`
/// `partial_cmp`), so both validity gates reject them up front (ticket 07).
fn has_non_finite_coords(geometry: &Geometry<f64>) -> bool {
    use geo::CoordsIter;
    geometry
        .coords_iter()
        .any(|coord| !coord.x.is_finite() || !coord.y.is_finite())
}

/// The geometry types supported for **rule** geometries (§2: Polygon and
/// MultiPolygon). Rules stay polygon-only even though candidates widened to
/// points (filtering-scale ticket 01); see `ensure_supported_candidate_geometry`.
pub fn ensure_supported_geometry(geometry: &Geometry<f64>) -> Result<(), SpatialError> {
    match geometry {
        Geometry::Polygon(_) | Geometry::MultiPolygon(_) => Ok(()),
        other => Err(unsupported_type_error(other)),
    }
}

/// The geometry types supported for **candidates**: Polygon, MultiPolygon,
/// Point, and MultiPoint (filtering-scale ticket 01).
fn ensure_supported_candidate_geometry(geometry: &Geometry<f64>) -> Result<(), SpatialError> {
    match geometry {
        Geometry::Polygon(_)
        | Geometry::MultiPolygon(_)
        | Geometry::Point(_)
        | Geometry::MultiPoint(_) => Ok(()),
        other => Err(unsupported_type_error(other)),
    }
}

/// The shared unsupported-type error, so the two gates construct it in one place.
fn unsupported_type_error(geometry: &Geometry<f64>) -> SpatialError {
    SpatialError::unsupported_geometry_type(format!(
        "unsupported geometry type: {}",
        geometry_type_name(geometry)
    ))
}

/// Strict gate for rule geometries: supported type AND OGC-valid (ADR-0005).
pub fn validate_rule_geometry(geometry: &Geometry<f64>) -> Result<(), SpatialError> {
    ensure_supported_geometry(geometry)?;
    if has_non_finite_coords(geometry) {
        return Err(SpatialError::invalid_geometry(
            "invalid rule geometry: non-finite coordinate",
        ));
    }
    if !geometry.is_valid() {
        return Err(SpatialError::invalid_geometry(format!(
            "invalid rule geometry: {:?}",
            geometry.validation_errors()
        )));
    }
    Ok(())
}

/// Classify a candidate geometry for the query pipeline (ADR-0005): it must use
/// a supported candidate type (Polygon, MultiPolygon, Point, or MultiPoint) AND
/// be OGC-valid AND have a bounding rectangle. Success returns the precomputed
/// envelope the spatial step needs; failure returns a structured
/// [`SpatialError`] so the caller can distinguish geometry categories (unsupported
/// type, non-finite coord, invalid geometry, no bounding rect) instead of
/// collapsing them to a `String`.
pub fn classify_candidate(geometry: &Geometry<f64>) -> Result<Rect<f64>, SpatialError> {
    #[cfg(test)]
    crate::test_support::record_classify_call();

    ensure_supported_candidate_geometry(geometry)?;
    if has_non_finite_coords(geometry) {
        return Err(SpatialError::invalid_geometry(
            "invalid geometry: non-finite coordinate",
        ));
    }
    if !geometry.is_valid() {
        return Err(SpatialError::invalid_geometry(format!(
            "invalid geometry: {:?}",
            geometry.validation_errors()
        )));
    }
    geometry.bounding_rect().ok_or_else(|| {
        SpatialError::invalid_geometry("geometry has no bounding rectangle")
    })
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
        use geo::Line;
        let error = classify_candidate(&Geometry::Line(Line::new(
            Point::new(0.0, 0.0),
            Point::new(1.0, 1.0),
        )))
        .unwrap_err();
        assert_eq!(error.code, crate::error::ErrorCode::UnsupportedGeometryType);
        assert_eq!(error.message, "unsupported geometry type: Line");
    }

    #[test]
    fn point_candidate_is_supported() {
        let rect = classify_candidate(&Geometry::Point(Point::new(5.0, 5.0))).unwrap();
        assert_eq!(rect, Rect::new((5.0, 5.0), (5.0, 5.0)));
    }

    #[test]
    fn multipoint_candidate_is_supported() {
        use geo::MultiPoint;
        let multipoint = MultiPoint::new(vec![Point::new(1.0, 1.0), Point::new(3.0, 3.0)]);
        let rect = classify_candidate(&Geometry::MultiPoint(multipoint)).unwrap();
        assert_eq!(rect, Rect::new((1.0, 1.0), (3.0, 3.0)));
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
        assert_eq!(reason.code, crate::error::ErrorCode::InvalidGeometry);
        assert!(reason.message.starts_with("invalid geometry:"));
    }

    #[test]
    fn non_finite_coords_are_rejected_not_panics() {
        let nan_square = Polygon::new(
            LineString::from(vec![
                (0.0, 0.0),
                (0.0, f64::NAN),
                (10.0, 10.0),
                (10.0, 0.0),
                (0.0, 0.0),
            ]),
            vec![],
        );

        let error = classify_candidate(&Geometry::Polygon(nan_square.clone())).unwrap_err();
        assert_eq!(error.code, crate::error::ErrorCode::InvalidGeometry);
        assert_eq!(error.message, "invalid geometry: non-finite coordinate");

        let err = validate_rule_geometry(&Geometry::Polygon(nan_square)).unwrap_err();
        assert_eq!(err.code, crate::error::ErrorCode::InvalidGeometry);
    }
}
