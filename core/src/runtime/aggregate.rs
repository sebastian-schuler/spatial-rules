//! Per-candidate analytics over the applicable rule set (ADR-0018).
//!
//! The [`Aggregate`] and [`AggregateSpec`] data types live in
//! [`crate::model::aggregate`] (so the domain `Query` type can own a spec
//! without depending upward); this module holds the parsing and computation,
//! and re-exports the data types.

use geo::{BooleanOps, GeodesicArea, Geometry, MultiPolygon};

use crate::runtime::access::RuleAccess;
use crate::model::candidate::Candidate;
use crate::error::SpatialError;
use crate::model::properties::PropertyValue;
use crate::model::rule::RuleId;

pub use crate::model::aggregate::{Aggregate, AggregateSpec};

impl AggregateSpec {
    /// Parse the `aggregate` query member. Strict: an object with unknown
    /// keys, a non-boolean `count`/`coverage`, a non-string numeric field, or
    /// nothing requested (`{}` or all-false) is `SR_INVALID_QUERY`.
    pub fn from_json(value: &serde_json::Value) -> Result<Self, SpatialError> {
        let object = value
            .as_object()
            .ok_or_else(|| SpatialError::invalid_query("'aggregate' must be an object"))?;
        for key in object.keys() {
            if !matches!(key.as_str(), "count" | "coverage" | "min" | "max" | "sum" | "avg") {
                return Err(SpatialError::invalid_query(format!(
                    "unknown aggregate function: '{key}'"
                )));
            }
        }
        let bool_member = |name: &str| -> Result<bool, SpatialError> {
            match object.get(name) {
                None => Ok(false),
                Some(value) => value.as_bool().ok_or_else(|| {
                    SpatialError::invalid_query(format!("'aggregate.{name}' must be a boolean"))
                }),
            }
        };
        let field_member = |name: &str| -> Result<Option<String>, SpatialError> {
            match object.get(name) {
                None => Ok(None),
                Some(value) => value.as_str().map(String::from).map(Some).ok_or_else(|| {
                    SpatialError::invalid_query(format!(
                        "'aggregate.{name}' must be a rule-property field name"
                    ))
                }),
            }
        };
        let spec = AggregateSpec {
            count: bool_member("count")?,
            min: field_member("min")?,
            max: field_member("max")?,
            sum: field_member("sum")?,
            avg: field_member("avg")?,
            coverage: bool_member("coverage")?,
        };
        if !spec.count
            && spec.min.is_none()
            && spec.max.is_none()
            && spec.sum.is_none()
            && spec.avg.is_none()
            && !spec.coverage
        {
            return Err(SpatialError::invalid_query(
                "'aggregate' must request at least one function",
            ));
        }
        Ok(spec)
    }

    /// Compute the requested aggregates over `applicable` — the candidate's
    /// applicable rule set (ADR-0015) — for a candidate (used for `coverage`).
    /// Each numeric field is `Some` only when the function was requested and at
    /// least one applicable rule contributes a numeric value; `coverage` is
    /// `Some` only when requested.
    pub fn compute<R: RuleAccess + ?Sized>(
        &self,
        candidate: &Candidate,
        applicable: &[RuleId],
        ruleset: &R,
    ) -> Aggregate {
        Aggregate {
            count: self.count.then_some(applicable.len() as u32),
            min: self.min.as_deref().and_then(|field| numeric(field, applicable, ruleset, NumericOp::Min)),
            max: self.max.as_deref().and_then(|field| numeric(field, applicable, ruleset, NumericOp::Max)),
            sum: self.sum.as_deref().and_then(|field| numeric(field, applicable, ruleset, NumericOp::Sum)),
            avg: self.avg.as_deref().and_then(|field| numeric(field, applicable, ruleset, NumericOp::Avg)),
            coverage: self.coverage.then(|| coverage_ratio(candidate, applicable, ruleset)),
        }
    }
}

/// The computed aggregate for one candidate. A field is `None` when its
/// function was not requested (or, for the numeric fields, when no applicable
/// rule contributed a numeric value), so serialization emits only requested
/// results.
#[derive(Clone, Copy)]
enum NumericOp {
    Min,
    Max,
    Sum,
    Avg,
}

/// The numeric (Int/Float) values of `field` across the applicable rules; a
/// rule whose property is missing or non-numeric is skipped (ADR-0018).
fn numeric_values<'a, R: RuleAccess + ?Sized>(
    field: &'a str,
    applicable: &'a [RuleId],
    ruleset: &'a R,
) -> impl Iterator<Item = f64> + 'a {
    applicable.iter().filter_map(move |&rule_id| match ruleset.properties(rule_id).get(field) {
        Some(PropertyValue::Int(value)) => Some(*value as f64),
        Some(PropertyValue::Float(value)) => Some(*value),
        _ => None,
    })
}

/// Apply a numeric aggregate to the field's values across the applicable rules;
/// `None` when no applicable rule contributes a numeric value.
fn numeric<'a, R: RuleAccess + ?Sized>(
    field: &'a str,
    applicable: &'a [RuleId],
    ruleset: &'a R,
    op: NumericOp,
) -> Option<f64> {
    let values = numeric_values(field, applicable, ruleset);
    match op {
        NumericOp::Min => {
            let mut values = values;
            let first = values.next()?;
            Some(values.fold(first, f64::min))
        }
        NumericOp::Max => {
            let mut values = values;
            let first = values.next()?;
            Some(values.fold(first, f64::max))
        }
        NumericOp::Sum | NumericOp::Avg => {
            // Share one (sum, count) fold; the projection is the only datum
            // that differs between Sum (the total) and Avg (total / count).
            let (sum, count) = numeric_sum_count(values);
            (count > 0).then(|| match op {
                NumericOp::Sum => sum,
                NumericOp::Avg => sum / count as f64,
                // MIN/MAX are handled above, so this arm is unreachable.
                NumericOp::Min | NumericOp::Max => unreachable!(),
            })
        }
    }
}

/// Fold an iterator of numeric values into `(sum, count)` in a single pass.
fn numeric_sum_count(values: impl Iterator<Item = f64>) -> (f64, usize) {
    let mut sum = 0.0;
    let mut count = 0;
    for value in values {
        sum += value;
        count += 1;
    }
    (sum, count)
}

/// Union coverage (ADR-0018): the fraction of the candidate's area covered by
/// the union of the applicable rules, via `BooleanOps::union` + `GeodesicArea`
/// (the same spherical machinery `overlap_metric` uses). Point/MultiPoint
/// candidates have zero area → `0`.
fn coverage_ratio<R: RuleAccess + ?Sized>(
    candidate: &Candidate,
    applicable: &[RuleId],
    ruleset: &R,
) -> f64 {
    if matches!(candidate.geometry(), Geometry::Point(_) | Geometry::MultiPoint(_)) {
        return 0.0;
    }
    let mut union: Option<MultiPolygon<f64>> = None;
    for &rule_id in applicable {
        let rule = match ruleset.geometry(rule_id) {
            Geometry::Polygon(polygon) => MultiPolygon::new(vec![polygon.clone()]),
            Geometry::MultiPolygon(multipolygon) => multipolygon.clone(),
            _ => unreachable!("rules are polygons"),
        };
        union = Some(match union {
            None => rule,
            Some(acc) => acc.union(&rule),
        });
    }
    let Some(union) = union else {
        return 0.0;
    };
    let intersection = match candidate.geometry() {
        Geometry::Polygon(polygon) => polygon.intersection(&union),
        Geometry::MultiPolygon(multipolygon) => multipolygon.intersection(&union),
        _ => unreachable!("coverage requires a polygon candidate"),
    };
    let covered_area = intersection.geodesic_area_signed().abs();
    let candidate_area = candidate.geometry().geodesic_area_signed().abs();
    if candidate_area > 0.0 {
        covered_area / candidate_area
    } else {
        0.0
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::model::candidate::Candidate;
    use crate::model::rule::Rule;
    use crate::runtime::ruleset::Ruleset;
    use geo::LineString;

    #[test]
    fn from_json_validates_strictly() {
        use crate::error::ErrorCode;
        let ok = AggregateSpec::from_json(&serde_json::json!({
            "count": true, "min": "speedLimit", "coverage": true
        }))
        .unwrap();
        assert!(ok.count);
        assert_eq!(ok.min.as_deref(), Some("speedLimit"));
        assert!(ok.avg.is_none());

        for bad in [
            serde_json::json!({ "median": true }),
            serde_json::json!({ "count": "yes" }),
            serde_json::json!({ "min": 5 }),
            serde_json::json!({}),
            serde_json::json!({ "count": false }),
            serde_json::json!("count"),
        ] {
            let err = AggregateSpec::from_json(&bad).unwrap_err();
            assert_eq!(err.code, ErrorCode::InvalidQuery, "{bad}");
        }
    }

    #[test]
    fn validate_rejects_an_empty_request() {
        // A programmatic all-false spec (which from_json rejects) must be caught
        // by validate so Query::validate surfaces it as Invalid.
        let empty = AggregateSpec {
            count: false,
            min: None,
            max: None,
            sum: None,
            avg: None,
            coverage: false,
        };
        assert!(empty.validate().is_some());

        for spec in [
            AggregateSpec { count: true, ..empty.clone() },
            AggregateSpec { coverage: true, ..empty.clone() },
            AggregateSpec { min: Some("m".to_string()), ..empty.clone() },
            AggregateSpec { sum: Some("s".to_string()), ..empty.clone() },
        ] {
            assert_eq!(spec.validate(), None);
        }
    }

    fn rule(id: &str, speed_limit: Option<i64>, tax_rate: Option<f64>) -> Rule {
        let mut properties = crate::model::properties::properties_from_json(&serde_json::Map::new());
        if let Some(v) = speed_limit {
            properties.insert("speedLimit".to_string(), PropertyValue::Int(v));
        }
        if let Some(v) = tax_rate {
            properties.insert("taxRate".to_string(), PropertyValue::Float(v));
        }
        Rule {
            id: id.to_string(),
            properties,
            geometry: Geometry::Polygon(geo::Polygon::new(
                LineString::from(vec![(0.0, 0.0), (0.0, 1.0), (1.0, 1.0), (1.0, 0.0), (0.0, 0.0)]),
                vec![],
            )),
            priority: 0,
        }
    }

    #[test]
    fn compute_folds_numeric_properties_and_skips_non_numeric() {
        let rules = vec![
            rule("a", Some(30), Some(0.21)),
            rule("b", None, Some(0.10)),
            rule("c", Some(50), None),
        ];
        let ruleset = Ruleset::build(rules).unwrap();
        let ids: Vec<RuleId> = (0..3).map(|index| RuleId::new(index, ruleset.id())).collect();
        let candidate = Candidate::new(
            "c".to_string(),
            Geometry::Polygon(geo::Polygon::new(
                LineString::from(vec![(0.2, 0.2), (0.2, 0.8), (0.8, 0.8), (0.8, 0.2), (0.2, 0.2)]),
                vec![],
            )),
        );

        let spec = AggregateSpec::from_json(&serde_json::json!({
            "count": true,
            "min": "speedLimit", "max": "speedLimit",
            "sum": "speedLimit", "avg": "speedLimit",
            "coverage": true
        }))
        .unwrap();
        let aggregate = spec.compute(&candidate, &ids, &ruleset);
        // rule "b" has no speedLimit -> skipped: 30 and 50 contribute.
        assert_eq!(aggregate.count, Some(3));
        assert_eq!(aggregate.min, Some(30.0));
        assert_eq!(aggregate.max, Some(50.0));
        assert_eq!(aggregate.sum, Some(80.0));
        assert_eq!(aggregate.avg, Some(40.0));
        // All three rules are the same square covering the candidate fully.
        let coverage = aggregate.coverage.unwrap();
        assert!((coverage - 1.0).abs() < 1e-6, "coverage {coverage}");
    }

    #[test]
    fn absent_fields_when_nothing_contributes_or_not_requested() {
        let ruleset = Ruleset::build(vec![rule("a", None, None)]).unwrap();
        let ids = vec![RuleId::new(0, 0)];
        let candidate = Candidate::new(
            "c".to_string(),
            Geometry::Polygon(geo::Polygon::new(
                LineString::from(vec![(0.2, 0.2), (0.2, 0.8), (0.8, 0.8), (0.8, 0.2), (0.2, 0.2)]),
                vec![],
            )),
        );
        let spec = AggregateSpec::from_json(&serde_json::json!({
            "count": true, "min": "speedLimit", "coverage": true
        }))
        .unwrap();
        let aggregate = spec.compute(&candidate, &ids, &ruleset);
        assert_eq!(aggregate.count, Some(1));
        assert_eq!(aggregate.min, None, "no numeric speedLimit contributes");
        assert_eq!(aggregate.max, None, "not requested");
        assert!(aggregate.coverage.is_some());
    }

    #[test]
    fn point_candidate_coverage_is_zero() {
        let ruleset = Ruleset::build(vec![rule("a", Some(10), None)]).unwrap();
        let ids = vec![RuleId::new(0, 0)];
        let point = Candidate::new("p".to_string(), Geometry::Point(geo::Point::new(0.5, 0.5)));
        let spec = AggregateSpec::from_json(&serde_json::json!({ "count": true, "coverage": true })).unwrap();
        let aggregate = spec.compute(&point, &ids, &ruleset);
        assert_eq!(aggregate.count, Some(1));
        assert_eq!(aggregate.coverage, Some(0.0));
    }
}