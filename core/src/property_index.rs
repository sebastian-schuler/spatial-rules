//! Equality index over rule properties, built at compile time (ADR-0003).
//!
//! `$in` is served by repeated equality lookups; range predicates are not
//! indexed and are scanned by the query engine. The indexability decision
//! lives in `where_expr` ([`IndexQuery`]); this module only implements the
//! lookup behind the [`PropertyIndex`] seam.

use std::collections::{BTreeMap, HashSet};

use crate::properties::PropertyValue;
use crate::rule::{Rule, RuleId};
use crate::where_expr::{FieldPredicate, IndexQuery, WhereExpr};

/// Answers property lookups from a compiled index (ADR-0003). A seam so the
/// index can be swapped (e.g. a future range index); one adapter today,
/// [`EqualityIndex`].
pub trait PropertyIndex: Send + Sync {
    /// The set of rule ids that can satisfy `where_clause` using only indexed
    /// lookups, or `None` when the clause needs per-rule evaluation.
    fn indexable_matches(&self, where_clause: &WhereExpr) -> Option<HashSet<RuleId>>;
}

/// Compile-time equality index: property name → (value → rule ids).
#[derive(Debug, Default)]
pub struct EqualityIndex {
    equality: BTreeMap<String, BTreeMap<PropertyValue, Vec<RuleId>>>,
}

impl EqualityIndex {
    /// Build the equality index from rules, assigning ids by position (`0..n-1`).
    pub fn build(rules: &[Rule]) -> Self {
        let mut equality: BTreeMap<String, BTreeMap<PropertyValue, Vec<RuleId>>> =
            BTreeMap::new();
        for (index, rule) in rules.iter().enumerate() {
            let rule_id = RuleId(index as u32);
            for (name, value) in &rule.properties {
                equality
                    .entry(name.clone())
                    .or_default()
                    .entry(value.clone())
                    .or_default()
                    .push(rule_id);
            }
        }
        EqualityIndex { equality }
    }

    /// Rule ids whose `name` property equals `value` (empty when none match).
    fn matching(&self, name: &str, value: &PropertyValue) -> &[RuleId] {
        self.equality
            .get(name)
            .and_then(|values| values.get(value))
            .map(|ids| ids.as_slice())
            .unwrap_or(&[])
    }

    /// Rule ids whose `name` property equals any of `values` — the `$in`
    /// operator. Union, sorted ascending, deduplicated.
    fn matching_in(&self, name: &str, values: &[PropertyValue]) -> Vec<RuleId> {
        let mut ids: Vec<RuleId> = Vec::new();
        for value in values {
            ids.extend_from_slice(self.matching(name, value));
        }
        ids.sort_unstable();
        ids.dedup();
        ids
    }

    fn predicate_matches(&self, predicate: &FieldPredicate) -> Option<HashSet<RuleId>> {
        match predicate.index_query()? {
            IndexQuery::Eq { field, value } => {
                Some(self.matching(field, value).iter().copied().collect())
            }
            IndexQuery::In { field, values } => Some(
                self.matching_in(field, values).into_iter().collect(),
            ),
        }
    }
}

impl PropertyIndex for EqualityIndex {
    fn indexable_matches(&self, where_clause: &WhereExpr) -> Option<HashSet<RuleId>> {
        match where_clause {
            WhereExpr::Predicate(predicate) => self.predicate_matches(predicate),
            WhereExpr::And(exprs) => {
                if exprs.is_empty() {
                    return None;
                }
                let mut result: Option<HashSet<RuleId>> = None;
                for expr in exprs {
                    let set = self.indexable_matches(expr)?;
                    result = Some(match result {
                        None => set,
                        Some(acc) => acc.intersection(&set).copied().collect(),
                    });
                }
                result
            }
            WhereExpr::Or(_) => None,
        }
    }
}
