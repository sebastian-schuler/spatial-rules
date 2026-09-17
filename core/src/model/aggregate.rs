//! Per-candidate analytics data definitions (ADR-0018).
//!
//! The [`AggregateSpec`] (a query's requested functions) and [`Aggregate`] (the
//! computed per-candidate result) are domain types: they model *what* the user
//! asked for and *what* the engine reports, and carry no computation. They live
//! in `model` so `Query` (also a domain type) can own an `AggregateSpec` without
//! depending upward on runtime. The computation lives in
//! [`crate::runtime::aggregate`], which imports these and implements
//! `from_json`/`compute`.

/// A query-level request for per-candidate aggregates (ADR-0018). `count` and
/// `coverage` are booleans; each numeric function names its own rule-property
/// field (Mongo `$min: "$field"` idiom).
#[derive(Debug, Clone, PartialEq)]
pub struct AggregateSpec {
    pub count: bool,
    pub min: Option<String>,
    pub max: Option<String>,
    pub sum: Option<String>,
    pub avg: Option<String>,
    pub coverage: bool,
}

impl AggregateSpec {
    /// Validate the invariant the JSON shape enforces so a programmatic
    /// [`AggregateSpec`] or [`Query`] cannot request nothing: at least one
    /// function (`count`, `coverage`, or a named numeric field) must be set.
    /// [`Query::validate`] consumes this so a programmatic query surfaces as
    /// `Invalid` rather than returning a [`Aggregate`] whose fields are all
    /// `None`.
    pub fn validate(&self) -> Option<&'static str> {
        if self.count
            || self.coverage
            || self.min.is_some()
            || self.max.is_some()
            || self.sum.is_some()
            || self.avg.is_some()
        {
            None
        } else {
            Some("'aggregate' must request at least one function")
        }
    }
}

/// The computed aggregate for one candidate. A field is `None` when its
/// function was not requested (or, for the numeric fields, when no applicable
/// rule contributed a numeric value), so serialization emits only requested
/// results.
#[derive(Debug, Clone, PartialEq)]
pub struct Aggregate {
    pub count: Option<u32>,
    pub min: Option<f64>,
    pub max: Option<f64>,
    pub sum: Option<f64>,
    pub avg: Option<f64>,
    pub coverage: Option<f64>,
}
