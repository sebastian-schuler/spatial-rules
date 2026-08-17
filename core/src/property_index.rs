//! Equality index over rule properties, built at compile time (ADR-0003).
//!
//! `$in` is served by repeated equality lookups; range predicates are not
//! indexed and are scanned by the query engine.

use std::collections::BTreeMap;

use crate::properties::PropertyValue;
use crate::rule::{Rule, RuleId};

/// Compile-time equality index: property name → (value → rule ids).
#[derive(Debug, Default)]
pub struct PropertyIndex {
    equality: BTreeMap<String, BTreeMap<PropertyValue, Vec<RuleId>>>,
}

impl PropertyIndex {
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
        PropertyIndex { equality }
    }

    /// Rule ids whose `name` property equals `value` (empty when none match).
    pub fn matching(&self, name: &str, value: &PropertyValue) -> &[RuleId] {
        self.equality
            .get(name)
            .and_then(|values| values.get(value))
            .map(|ids| ids.as_slice())
            .unwrap_or(&[])
    }

    /// Rule ids whose `name` property equals any of `values` — the `$in`
    /// operator. Union, sorted ascending, deduplicated.
    pub fn matching_in(&self, name: &str, values: &[PropertyValue]) -> Vec<RuleId> {
        let mut ids: Vec<RuleId> = Vec::new();
        for value in values {
            ids.extend_from_slice(self.matching(name, value));
        }
        ids.sort_unstable();
        ids.dedup();
        ids
    }

    /// Number of property names with a built index.
    pub fn len(&self) -> usize {
        self.equality.len()
    }

    /// Whether no property has been indexed.
    pub fn is_empty(&self) -> bool {
        self.equality.is_empty()
    }

    /// Indexed property names.
    pub fn property_names(&self) -> impl Iterator<Item = &str> {
        self.equality.keys().map(|name| name.as_str())
    }
}
