//! Mongo-style `where` AST: parsing and evaluation (ADR-0003, ADR-0011).
//!
//! Subset: implicit top-level `AND`, plain-value equality, `$eq`, `$ne`,
//! `$gt/$gte/$lt/$lte`, `$in`, `$nin`, `$exists`, field-level `$not`,
//! `$and`/`$or`, whole-clause `$nor`, and the whole-clause temporal
//! `$activeAt` (ADR-0017). A missing property or a type mismatch evaluates as
//! non-match (even for `$ne`); only malformed predicates error.

use std::cmp::Ordering;
use std::collections::BTreeMap;

use crate::error::SpatialError;
use crate::model::properties::PropertyValue;
use crate::model::temporal::TemporalInstant;

/// A boolean property predicate tree.
#[derive(Debug, Clone, PartialEq)]
pub enum WhereExpr {
    And(Vec<WhereExpr>),
    Or(Vec<WhereExpr>),
    /// `$nor`: matches when none of the inner clauses match — whole-clause
    /// negation (filtering-scale ticket 02).
    Nor(Vec<WhereExpr>),
    Predicate(FieldPredicate),
    /// `$activeAt`: the rule's window fields contain the query-supplied
    /// reference time (ADR-0017). The fields are named explicitly, so no
    /// rule-schema key is reserved.
    ActiveAt(ActiveAtClause),
}

/// The rule window a `$activeAt` predicate tests (ADR-0017): a `daysOfWeek`
/// Int bitmask (Mon=1 … Sun=64) and `startHour`/`endHour` Int hours
/// (0..=23), start-inclusive / end-exclusive with midnight wrap.
#[derive(Debug, Clone, PartialEq)]
pub struct ActiveAtClause {
    pub days_of_week_field: String,
    pub start_hour_field: String,
    pub end_hour_field: String,
}

/// A single field predicate: `field OP value`.
#[derive(Debug, Clone, PartialEq)]
pub struct FieldPredicate {
    pub field: String,
    pub op: FieldOp,
}

/// A comparison against one field.
#[derive(Debug, Clone, PartialEq)]
pub enum FieldOp {
    Eq(PropertyValue),
    Ne(PropertyValue),
    Gt(PropertyValue),
    Gte(PropertyValue),
    Lt(PropertyValue),
    Lte(PropertyValue),
    In(Vec<PropertyValue>),
    /// Field-level `$not`: negates exactly one inner field predicate (ADR-0011).
    Not(Box<FieldPredicate>),
    /// `$nin`: present, same-typed, and not equal to any listed value (ADR-0011).
    Nin(Vec<PropertyValue>),
    /// `$exists`: key presence check (ADR-0011).
    Exists(bool),
}

/// The form of a predicate an index can answer directly (ADR-0003). `where_expr`
/// owns the operator semantics; indexes consume this instead of re-enumerating
/// [`FieldOp`] variants.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum IndexQuery<'a> {
    /// `field = value` — an equality lookup.
    Eq {
        field: &'a str,
        value: &'a PropertyValue,
    },
    /// `field IN values` — repeated equality lookups.
    In {
        field: &'a str,
        values: &'a [PropertyValue],
    },
}

impl WhereExpr {
    /// Parse a Mongo-style `where` object into an AST.
    pub fn parse(value: &serde_json::Value) -> Result<Self, SpatialError> {
        let object = value
            .as_object()
            .ok_or_else(|| invalid_predicate("where must be an object"))?;

        if object.len() == 1 {
            let (key, value) = object.iter().next().unwrap();
            match key.as_str() {
                "$and" => Ok(WhereExpr::And(parse_expr_list(value)?)),
                "$or" => Ok(WhereExpr::Or(parse_expr_list(value)?)),
                "$nor" => Ok(WhereExpr::Nor(parse_expr_list(value)?)),
                "$activeAt" => Ok(WhereExpr::ActiveAt(parse_active_at(value)?)),
                _ if key.starts_with('$') => Err(SpatialError::unsupported_property_operator(
                    format!("unsupported operator: {key}"),
                )),
                _ => parse_implicit_and(object),
            }
        } else {
            parse_implicit_and(object)
        }
    }

    /// Whether any `$activeAt` clause appears anywhere in the tree — the query
    /// validator uses this to require the reference time (`at`, ADR-0017).
    pub fn has_active_at(&self) -> bool {
        match self {
            WhereExpr::ActiveAt(_) => true,
            WhereExpr::And(exprs) | WhereExpr::Or(exprs) | WhereExpr::Nor(exprs) => {
                exprs.iter().any(WhereExpr::has_active_at)
            }
            WhereExpr::Predicate(_) => false,
        }
    }

    /// Evaluate against a rule's properties. `at` is the query's reference time
    /// for temporal predicates (ADR-0017); it is `None` only when the query
    /// carries none, in which case any `$activeAt` clause is a non-match (the
    /// query validator prevents this combination).
    pub fn eval(&self, properties: &BTreeMap<String, PropertyValue>, at: Option<TemporalInstant>) -> bool {
        match self {
            WhereExpr::And(exprs) => exprs.iter().all(|expr| expr.eval(properties, at)),
            WhereExpr::Or(exprs) => exprs.iter().any(|expr| expr.eval(properties, at)),
            WhereExpr::Nor(exprs) => !exprs.iter().any(|expr| expr.eval(properties, at)),
            WhereExpr::Predicate(predicate) => predicate.eval(properties),
            WhereExpr::ActiveAt(clause) => eval_active_at(clause, properties, at),
        }
    }
}

impl FieldPredicate {
    fn eval(&self, properties: &BTreeMap<String, PropertyValue>) -> bool {
        match &self.op {
            FieldOp::Eq(expected) => properties.get(&self.field) == Some(expected),
            FieldOp::Ne(expected) => match properties.get(&self.field) {
                // `$ne` requires the property to exist with the same type;
                // a missing property or type mismatch is a non-match.
                Some(actual) => same_variant(actual, expected) && actual != expected,
                None => false,
            },
            FieldOp::Gt(expected) => compares(properties, &self.field, expected, Ordering::Greater),
            FieldOp::Gte(expected) => {
                compares_inclusive(properties, &self.field, expected, Ordering::Greater)
            }
            FieldOp::Lt(expected) => compares(properties, &self.field, expected, Ordering::Less),
            FieldOp::Lte(expected) => {
                compares_inclusive(properties, &self.field, expected, Ordering::Less)
            }
            FieldOp::In(values) => match properties.get(&self.field) {
                Some(actual) => values.iter().any(|value| actual == value),
                None => false,
            },
            FieldOp::Not(inner) => !inner.eval(properties),
            FieldOp::Nin(values) => match properties.get(&self.field) {
                // A missing field or a type mismatch (no same-variant list
                // element) is a non-match — a documented divergence from Mongo
                // (ADR-0011).
                Some(actual) => {
                    values.iter().any(|value| same_variant(actual, value))
                        && !values.iter().any(|value| actual == value)
                }
                None => false,
            },
            FieldOp::Exists(expected) => {
                properties.contains_key(&self.field) == *expected
            }
        }
    }

    /// The index-answerable form of this predicate, or `None` when no equality
    /// index can answer it (`$ne`, ranges). The single owner of "what is
    /// indexable" — indexes consume this instead of matching on [`FieldOp`].
    pub(crate) fn index_query(&self) -> Option<IndexQuery<'_>> {
        match &self.op {
            FieldOp::Eq(value) => Some(IndexQuery::Eq {
                field: &self.field,
                value,
            }),
            FieldOp::In(values) => Some(IndexQuery::In {
                field: &self.field,
                values,
            }),
            _ => None,
        }
    }
}

/// Whether `properties[field]` compares to `expected` with `ordering`
/// (numeric comparison only; a non-numeric side is a non-match).
fn compares(
    properties: &BTreeMap<String, PropertyValue>,
    field: &str,
    expected: &PropertyValue,
    ordering: Ordering,
) -> bool {
    properties
        .get(field)
        .and_then(|actual| compare_numbers(actual, expected))
        .is_some_and(|found| found == ordering)
}

/// Inclusive comparison (`>=`/`<=`): resolve the actual-vs-expected ordering
/// once, then admit `Equal` or the strict `ordering`. Computing the ordering
/// once avoids re-looking-up the field and re-converting the number for the
/// equality half of `Gte`/`Lte`.
fn compares_inclusive(
    properties: &BTreeMap<String, PropertyValue>,
    field: &str,
    expected: &PropertyValue,
    ordering: Ordering,
) -> bool {
    properties
        .get(field)
        .and_then(|actual| compare_numbers(actual, expected))
        .is_some_and(|found| found == ordering || found == Ordering::Equal)
}

/// Numeric comparison across `Int`/`Float`; `None` when either side is not
/// numeric. Mixed Int/Float compares as `f64` (precision loss only beyond 2^53,
/// outside the v1 property domain).
fn compare_numbers(a: &PropertyValue, b: &PropertyValue) -> Option<Ordering> {
    match (a, b) {
        (PropertyValue::Int(x), PropertyValue::Int(y)) => Some(x.cmp(y)),
        (PropertyValue::Float(x), PropertyValue::Float(y)) => x.partial_cmp(y),
        (PropertyValue::Int(x), PropertyValue::Float(y)) => (*x as f64).partial_cmp(y),
        (PropertyValue::Float(x), PropertyValue::Int(y)) => x.partial_cmp(&(*y as f64)),
        _ => None,
    }
}

fn same_variant(a: &PropertyValue, b: &PropertyValue) -> bool {
    std::mem::discriminant(a) == std::mem::discriminant(b)
}

fn parse_expr_list(value: &serde_json::Value) -> Result<Vec<WhereExpr>, SpatialError> {
    let array = value
        .as_array()
        .ok_or_else(|| invalid_predicate("$and/$or/$nor requires an array of predicates"))?;
    array.iter().map(WhereExpr::parse).collect()
}

/// Parse `{ "$activeAt": { "daysOfWeek": <field>, "startHour": <field>, "endHour": <field> } }`
/// (ADR-0017). The values are the rule property names that declare the window;
/// no rule-schema key is reserved.
fn parse_active_at(value: &serde_json::Value) -> Result<ActiveAtClause, SpatialError> {
    let map = value
        .as_object()
        .ok_or_else(|| invalid_predicate("$activeAt requires an object of window field names"))?;
    let field = |name: &str| -> Result<String, SpatialError> {
        map.get(name)
            .and_then(|value| value.as_str())
            .map(String::from)
            .ok_or_else(|| {
                invalid_predicate(format!("$activeAt requires a string '{name}' field name"))
            })
    };
    Ok(ActiveAtClause {
        days_of_week_field: field("daysOfWeek")?,
        start_hour_field: field("startHour")?,
        end_hour_field: field("endHour")?,
    })
}

/// A `$activeAt` admission (ADR-0017): the rule's `daysOfWeek` bitmask
/// (Mon=1 … Sun=64) contains the reference day and its hour falls in
/// `[startHour, endHour)` (midnight-wrapping when `startHour > endHour`). A
/// missing or non-Int temporal field is a non-match; `daysOfWeek = 0` never
/// admits; `startHour == endHour` is an empty window.
fn eval_active_at(
    clause: &ActiveAtClause,
    properties: &BTreeMap<String, PropertyValue>,
    at: Option<TemporalInstant>,
) -> bool {
    let Some(at) = at else {
        return false;
    };
    let get_int = |field: &str| match properties.get(field) {
        Some(PropertyValue::Int(value)) => Some(*value),
        _ => None,
    };
    let Some(days) = get_int(&clause.days_of_week_field) else {
        return false;
    };
    let Some(start) = get_int(&clause.start_hour_field) else {
        return false;
    };
    let Some(end) = get_int(&clause.end_hour_field) else {
        return false;
    };
    let day = at.day_of_week();
    // The type enforces day 1..=7 at construction; guard anyway so a malformed
    // value (were one ever constructible) evaluates as a non-match, never
    // underflowing the bit shift below (no-panic stance).
    if !(1..=7).contains(&day) {
        return false;
    }
    if days & (1 << (day - 1)) == 0 {
        return false;
    }
    let hour = at.hour() as i64;
    if start <= end {
        start <= hour && hour < end
    } else {
        hour >= start || hour < end
    }
}

fn parse_implicit_and(
    object: &serde_json::Map<String, serde_json::Value>,
) -> Result<WhereExpr, SpatialError> {
    let mut predicates = Vec::new();
    for (field, value) in object {
        if field.starts_with('$') {
            return Err(SpatialError::unsupported_property_operator(format!(
                "unsupported operator: {field}"
            )));
        }
        predicates.push(parse_field_predicate(field, value)?);
    }
    Ok(match predicates.len() {
        0 => WhereExpr::And(Vec::new()),
        1 => predicates.pop().unwrap(),
        _ => WhereExpr::And(predicates),
    })
}

fn parse_field_predicate(
    field: &str,
    value: &serde_json::Value,
) -> Result<WhereExpr, SpatialError> {
    match value {
        serde_json::Value::Object(operators) => {
            if operators.len() != 1 {
                return Err(invalid_predicate(format!(
                    "predicate for '{field}' must have exactly one operator"
                )));
            }
            let (operator, operand) = operators.iter().next().unwrap();
            let op = parse_field_op(field, operator, operand)?;
            Ok(WhereExpr::Predicate(FieldPredicate {
                field: field.to_string(),
                op,
            }))
        }
        serde_json::Value::Array(_) => Err(invalid_predicate(format!(
            "array value for '{field}' is only valid inside $in"
        ))),
        scalar => Ok(WhereExpr::Predicate(FieldPredicate {
            field: field.to_string(),
            op: FieldOp::Eq(parse_scalar(scalar)?),
        })),
    }
}

fn parse_scalar(value: &serde_json::Value) -> Result<PropertyValue, SpatialError> {
    PropertyValue::from_json_value(value)
        .ok_or_else(|| invalid_predicate("expected a scalar property value"))
}

fn parse_array(value: &serde_json::Value) -> Result<Vec<PropertyValue>, SpatialError> {
    let array = value
        .as_array()
        .ok_or_else(|| invalid_predicate("$in/$nin requires an array"))?;
    array.iter().map(parse_scalar).collect()
}

fn parse_bool(value: &serde_json::Value) -> Result<bool, SpatialError> {
    value
        .as_bool()
        .ok_or_else(|| invalid_predicate("$exists requires a boolean"))
}

/// Parse a single `$operator: operand` pair for `field` into a [`FieldOp`].
/// The single dispatch point for every field operator, including the `$not`
/// wrapper and its recursive inner operator.
fn parse_field_op(
    field: &str,
    operator: &str,
    operand: &serde_json::Value,
) -> Result<FieldOp, SpatialError> {
    match operator {
        "$eq" => Ok(FieldOp::Eq(parse_scalar(operand)?)),
        "$ne" => Ok(FieldOp::Ne(parse_scalar(operand)?)),
        "$gt" => Ok(FieldOp::Gt(parse_scalar(operand)?)),
        "$gte" => Ok(FieldOp::Gte(parse_scalar(operand)?)),
        "$lt" => Ok(FieldOp::Lt(parse_scalar(operand)?)),
        "$lte" => Ok(FieldOp::Lte(parse_scalar(operand)?)),
        "$in" => Ok(FieldOp::In(parse_array(operand)?)),
        "$nin" => Ok(FieldOp::Nin(parse_array(operand)?)),
        "$exists" => Ok(FieldOp::Exists(parse_bool(operand)?)),
        "$not" => Ok(FieldOp::Not(Box::new(parse_not_inner(field, operand)?))),
        other if other.starts_with('$') => Err(SpatialError::unsupported_property_operator(
            format!("unsupported operator: {other}"),
        )),
        _ => Err(invalid_predicate(format!(
            "predicate for '{field}' must use an operator like $gt or a plain value"
        ))),
    }
}

/// Parse the inner predicate of a `$not`: exactly one field operator, which may
/// itself be another `$not` (nesting), on the same `field` (ADR-0011).
fn parse_not_inner(
    field: &str,
    operand: &serde_json::Value,
) -> Result<FieldPredicate, SpatialError> {
    let object = operand
        .as_object()
        .ok_or_else(|| invalid_predicate("$not requires an object with exactly one inner operator"))?;
    if object.len() != 1 {
        return Err(invalid_predicate("$not requires exactly one inner operator"));
    }
    let (operator, operand) = object.iter().next().unwrap();
    let op = parse_field_op(field, operator, operand)?;
    Ok(FieldPredicate {
        field: field.to_string(),
        op,
    })
}

fn invalid_predicate(message: impl Into<String>) -> SpatialError {
    SpatialError::invalid_property_predicate(message)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn predicate(field: &str, value: serde_json::Value) -> FieldPredicate {
        let mut object = serde_json::Map::new();
        object.insert(field.to_string(), value);
        match WhereExpr::parse(&serde_json::Value::Object(object)).unwrap() {
            WhereExpr::Predicate(predicate) => predicate,
            _ => panic!("expected a single predicate"),
        }
    }

    #[test]
    fn equality_and_in_are_indexable() {
        let eq = predicate("country", serde_json::json!("HR"));
        assert!(matches!(
            eq.index_query(),
            Some(IndexQuery::Eq {
                field: "country",
                value: PropertyValue::Str(_)
            })
        ));

        let in_pred = predicate("country", serde_json::json!({ "$in": ["HR", "SI"] }));
        assert!(matches!(
            in_pred.index_query(),
            Some(IndexQuery::In {
                field: "country",
                values
            }) if values.len() == 2
        ));
    }

    #[test]
    fn ne_and_ranges_are_not_indexable() {
        assert!(predicate("priority", serde_json::json!({ "$ne": 10 }))
            .index_query()
            .is_none());
        assert!(predicate("priority", serde_json::json!({ "$gt": 5 }))
            .index_query()
            .is_none());
        assert!(predicate("priority", serde_json::json!({ "$gte": 5 }))
            .index_query()
            .is_none());
        assert!(predicate("priority", serde_json::json!({ "$lt": 5 }))
            .index_query()
            .is_none());
        assert!(predicate("priority", serde_json::json!({ "$lte": 5 }))
            .index_query()
            .is_none());
    }

    #[test]
    fn eq_operator_parses_as_plain_equality() {
        let eq = predicate("country", serde_json::json!({ "$eq": "HR" }));
        assert_eq!(eq.op, FieldOp::Eq(PropertyValue::Str("HR".into())));
        assert!(eq.index_query().is_some());
    }

    #[test]
    fn new_operators_are_not_indexable() {
        // No index extension (ADR-0011): the new operators answer per-rule.
        assert!(predicate("priority", serde_json::json!({ "$nin": [5] }))
            .index_query()
            .is_none());
        assert!(predicate("priority", serde_json::json!({ "$exists": true }))
            .index_query()
            .is_none());
        assert!(predicate("priority", serde_json::json!({ "$not": { "$eq": 5 } }))
            .index_query()
            .is_none());
    }

    #[test]
    fn not_requires_a_single_inner_operator() {
        let err = predicate_err("active", serde_json::json!({ "$not": true }));
        assert_eq!(err.code, crate::error::ErrorCode::InvalidPropertyPredicate);

        let err = predicate_err("active", serde_json::json!({ "$not": { "$eq": 1, "$ne": 2 } }));
        assert_eq!(err.code, crate::error::ErrorCode::InvalidPropertyPredicate);
    }

    fn predicate_err(field: &str, value: serde_json::Value) -> crate::error::SpatialError {
        let mut object = serde_json::Map::new();
        object.insert(field.to_string(), value);
        WhereExpr::parse(&serde_json::Value::Object(object)).unwrap_err()
    }
}
