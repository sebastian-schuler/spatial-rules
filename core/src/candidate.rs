//! Candidate type — a geometry being evaluated against the rules.

use geo::{Geometry, Rect};

/// The classification of a candidate, computed once at intake
/// (architecture-hardening 01): a valid candidate carries its bounding
/// envelope; an invalid candidate carries the reason (ADR-0005). The query hot
/// path reads this instead of re-running OGC validation + envelope derivation
/// on every query.
#[derive(Debug, Clone, PartialEq)]
pub enum CandidateClass {
    /// A supported, OGC-valid candidate geometry and its bounding envelope.
    Valid { envelope: Rect<f64> },
    /// The reason the candidate is invalid (unsupported type, non-finite
    /// coordinate, invalid geometry, or no bounding rectangle).
    Invalid { reason: String },
}

/// A candidate geometry under evaluation (CONTEXT.md §4.2).
///
/// Only the geometry participates in matching; candidate properties are not
/// used by the engine in v1.
#[derive(Debug, Clone, PartialEq)]
pub struct Candidate {
    /// Application-supplied identifier (feature `id`).
    pub id: String,
    /// The candidate geometry (Polygon, MultiPolygon, Point, or MultiPoint).
    pub geometry: Geometry<f64>,
    /// The precomputed classification (envelope or invalid reason).
    class: CandidateClass,
}

impl Candidate {
    /// Classify a candidate at intake: compute the envelope for a valid
    /// geometry, or record the reason it is invalid. Never fails — an invalid
    /// candidate is stored as such and reported per query (ADR-0005).
    pub fn new(id: String, geometry: Geometry<f64>) -> Self {
        let class = match crate::validation::classify_candidate(&geometry) {
            Ok(envelope) => CandidateClass::Valid { envelope },
            Err(reason) => CandidateClass::Invalid { reason },
        };
        Candidate {
            id,
            geometry,
            class,
        }
    }

    /// The precomputed classification (envelope for valid candidates, invalid
    /// reason otherwise), consumed by the query hot path.
    pub fn class(&self) -> &CandidateClass {
        &self.class
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::query::{Query, SpatialPredicate};
    use crate::rule::Rule;
    use crate::ruleset::Ruleset;
    use geo::{LineString, Polygon};

    #[test]
    fn validation_runs_once_at_intake_not_per_query() {
        let geometry = Geometry::Polygon(Polygon::new(
            LineString::from(vec![
                (0.0, 0.0),
                (0.0, 10.0),
                (10.0, 10.0),
                (10.0, 0.0),
                (0.0, 0.0),
            ]),
            vec![],
        ));
        let ruleset = Ruleset::build(vec![Rule {
            id: "zone".to_string(),
            properties: Default::default(),
            geometry: geometry.clone(),
            priority: 0,
        }])
        .unwrap();
        let query = Query::new(SpatialPredicate::Intersects);

        let before = crate::test_support::classify_call_count();
        let candidate = Candidate::new("c".to_string(), geometry);
        // Classification ran exactly once, at intake.
        assert_eq!(crate::test_support::classify_call_count(), before + 1);

        // Re-querying the same classified candidate never re-runs validation.
        for _ in 0..100 {
            let _ = ruleset.query(std::slice::from_ref(&candidate), &query);
        }
        assert_eq!(crate::test_support::classify_call_count(), before + 1);
    }

    #[test]
    fn invalid_candidate_carries_reason_at_intake() {
        let bowtie = Geometry::Polygon(Polygon::new(
            LineString::from(vec![
                (0.0, 0.0),
                (10.0, 10.0),
                (0.0, 10.0),
                (10.0, 0.0),
                (0.0, 0.0),
            ]),
            vec![],
        ));
        let candidate = Candidate::new("bowtie".to_string(), bowtie);
        match candidate.class() {
            CandidateClass::Invalid { reason } => {
                assert!(reason.starts_with("invalid geometry:"));
            }
            CandidateClass::Valid { .. } => panic!("bowtie must classify invalid"),
        }
    }
}
