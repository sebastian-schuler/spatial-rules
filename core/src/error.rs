//! Structured error model for the engine (ADR-0005).
//!
//! Errors carry a stable `SR_*` code plus a human-readable message. The Node
//! binding maps these to a `SpatialRulesError` with a `.code` property; the
//! core itself never touches JavaScript.

use std::fmt;

/// Stable error code, rendered as an `SR_*` string (ADR-0005, §35).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum ErrorCode {
    /// Malformed or unparseable GeoJSON.
    InvalidGeoJson,
    /// A rule geometry failed OGC validity (see `geo::Validation`).
    InvalidGeometry,
    /// A structurally invalid query.
    InvalidQuery,
    /// A malformed property predicate in a query's `where` clause.
    InvalidPropertyPredicate,
    /// Ruleset construction failed (an aggregate of per-rule failures).
    RulesetConstructionFailed,
    /// A geometry type outside the supported `Polygon`/`MultiPolygon` set.
    UnsupportedGeometryType,
    /// A spatial predicate other than `intersects`/`contains`/`within`.
    UnsupportedSpatialPredicate,
    /// A property operator outside the supported Mongo-style subset.
    UnsupportedPropertyOperator,
    /// An unexpected native/runtime failure.
    Native,
}

impl ErrorCode {
    /// The canonical `SR_*` string for this code.
    pub fn as_str(self) -> &'static str {
        match self {
            ErrorCode::InvalidGeoJson => "SR_INVALID_GEOJSON",
            ErrorCode::InvalidGeometry => "SR_INVALID_GEOMETRY",
            ErrorCode::InvalidQuery => "SR_INVALID_QUERY",
            ErrorCode::InvalidPropertyPredicate => "SR_INVALID_PROPERTY_PREDICATE",
            ErrorCode::RulesetConstructionFailed => "SR_RULESET_CONSTRUCTION_FAILED",
            ErrorCode::UnsupportedGeometryType => "SR_UNSUPPORTED_GEOMETRY_TYPE",
            ErrorCode::UnsupportedSpatialPredicate => "SR_UNSUPPORTED_SPATIAL_PREDICATE",
            ErrorCode::UnsupportedPropertyOperator => "SR_UNSUPPORTED_PROPERTY_OPERATOR",
            ErrorCode::Native => "SR_NATIVE",
        }
    }
}

impl fmt::Display for ErrorCode {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(self.as_str())
    }
}

/// A structured engine error: a stable code plus a message (ADR-0005).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SpatialError {
    pub code: ErrorCode,
    pub message: String,
}

impl SpatialError {
    pub fn new(code: ErrorCode, message: impl Into<String>) -> Self {
        SpatialError {
            code,
            message: message.into(),
        }
    }

    pub fn invalid_geojson(message: impl Into<String>) -> Self {
        Self::new(ErrorCode::InvalidGeoJson, message)
    }

    pub fn invalid_geometry(message: impl Into<String>) -> Self {
        Self::new(ErrorCode::InvalidGeometry, message)
    }

    pub fn unsupported_geometry_type(message: impl Into<String>) -> Self {
        Self::new(ErrorCode::UnsupportedGeometryType, message)
    }
}

impl fmt::Display for SpatialError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}: {}", self.code, self.message)
    }
}

impl std::error::Error for SpatialError {}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn error_codes_render_stable_sr_strings() {
        assert_eq!(ErrorCode::InvalidGeoJson.as_str(), "SR_INVALID_GEOJSON");
        assert_eq!(ErrorCode::InvalidGeometry.as_str(), "SR_INVALID_GEOMETRY");
        assert_eq!(ErrorCode::InvalidQuery.as_str(), "SR_INVALID_QUERY");
        assert_eq!(
            ErrorCode::InvalidPropertyPredicate.as_str(),
            "SR_INVALID_PROPERTY_PREDICATE"
        );
        assert_eq!(
            ErrorCode::RulesetConstructionFailed.as_str(),
            "SR_RULESET_CONSTRUCTION_FAILED"
        );
        assert_eq!(
            ErrorCode::UnsupportedGeometryType.as_str(),
            "SR_UNSUPPORTED_GEOMETRY_TYPE"
        );
        assert_eq!(
            ErrorCode::UnsupportedSpatialPredicate.as_str(),
            "SR_UNSUPPORTED_SPATIAL_PREDICATE"
        );
        assert_eq!(
            ErrorCode::UnsupportedPropertyOperator.as_str(),
            "SR_UNSUPPORTED_PROPERTY_OPERATOR"
        );
        assert_eq!(ErrorCode::Native.as_str(), "SR_NATIVE");
    }

    #[test]
    fn spatial_error_displays_code_and_message() {
        let err = SpatialError::invalid_geometry("bad ring");
        assert_eq!(err.to_string(), "SR_INVALID_GEOMETRY: bad ring");
    }
}
