//! Mongo-style `where` AST: parsing and evaluation (ADR-0003).
//!
//! Subset: implicit top-level `AND`, plain-value equality, `$ne`,
//! `$gt/$gte/$lt/$lte`, `$in`, and `$and`/`$or`. A missing property or a type
//! mismatch evaluates as non-match (even for `$ne`); only malformed predicates
//! error.

use std::cmp::Ordering;
use std::collections::BTreeMap;

use crate::error::SpatialError;
use crate::properties::PropertyValue;

/// A boolean property predicate tree.
#[derive(Debug, Clone, PartialEq)]
pub enum WhereExpr {
    And(Vec<WhereExpr>),
    Or(Vec<WhereExpr>),
    Predicate(FieldPredicate),
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
                _ if key.starts_with('$') => Err(SpatialError::unsupported_property_operator(
                    format!("unsupported operator: {key}"),
                )),
                _ => parse_implicit_and(object),
            }
        } else {
            parse_implicit_and(object)
        }
    }

    /// Evaluate against a rule's properties.
    pub fn eval(&self, properties: &BTreeMap<String, PropertyValue>) -> bool {
        match self {
            WhereExpr::And(exprs) => exprs.iter().all(|expr| expr.eval(properties)),
            WhereExpr::Or(exprs) => exprs.iter().any(|expr| expr.eval(properties)),
            WhereExpr::Predicate(predicate) => predicate.eval(properties),
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
                compares(properties, &self.field, expected, Ordering::Greater)
                    || compares(properties, &self.field, expected, Ordering::Equal)
            }
            FieldOp::Lt(expected) => compares(properties, &self.field, expected, Ordering::Less),
            FieldOp::Lte(expected) => {
                compares(properties, &self.field, expected, Ordering::Less)
                    || compares(properties, &self.field, expected, Ordering::Equal)
            }
            FieldOp::In(values) => match properties.get(&self.field) {
                Some(actual) => values.iter().any(|value| actual == value),
                None => false,
            },
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
        .ok_or_else(|| invalid_predicate("$and/$or requires an array of predicates"))?;
    array.iter().map(WhereExpr::parse).collect()
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
            let op = match operator.as_str() {
                "$ne" => FieldOp::Ne(parse_scalar(operand)?),
                "$gt" => FieldOp::Gt(parse_scalar(operand)?),
                "$gte" => FieldOp::Gte(parse_scalar(operand)?),
                "$lt" => FieldOp::Lt(parse_scalar(operand)?),
                "$lte" => FieldOp::Lte(parse_scalar(operand)?),
                "$in" => FieldOp::In(parse_array(operand)?),
                other if other.starts_with('$') => {
                    return Err(SpatialError::unsupported_property_operator(format!(
                        "unsupported operator: {other}"
                    )));
                }
                _ => {
                    return Err(invalid_predicate(format!(
                        "predicate for '{field}' must use an operator like $gt or a plain value"
                    )));
                }
            };
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
        .ok_or_else(|| invalid_predicate("$in requires an array"))?;
    array.iter().map(parse_scalar).collect()
}

fn invalid_predicate(message: impl Into<String>) -> SpatialError {
    SpatialError::invalid_property_predicate(message)
}
