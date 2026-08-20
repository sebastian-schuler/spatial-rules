//! Thread-local storage for prepared rule geometries (ADR-0010).

use std::cell::RefCell;
use std::rc::Rc;
use std::sync::atomic::{AtomicU64, Ordering};

use geo::{Geometry, PreparedGeometry};

use crate::rule::Rule;

/// Owned prepared geometries for one ruleset, shared per thread via `Rc`.
pub(crate) type PreparedGeometries = Rc<Vec<PreparedGeometry<'static, Geometry<f64>>>>;

/// Assigns each `Ruleset` a unique identity, used as the per-thread cache key.
static NEXT_RULESET_ID: AtomicU64 = AtomicU64::new(1);

thread_local! {
    static PREPARED_CACHE: RefCell<Option<(u64, PreparedGeometries)>> = const { RefCell::new(None) };
}

pub(crate) fn next_ruleset_id() -> u64 {
    NEXT_RULESET_ID.fetch_add(1, Ordering::Relaxed)
}

/// Return the prepared geometries for `ruleset_id`, preparing them once on the
/// current thread and replacing the stale cache entry when the ruleset changes.
pub(crate) fn get_or_prepare(rules: &[Rule], ruleset_id: u64) -> PreparedGeometries {
    PREPARED_CACHE.with(|cache| {
        {
            let cached = cache.borrow();
            if let Some((id, prepared)) = cached.as_ref() {
                if *id == ruleset_id {
                    return prepared.clone();
                }
            }
        }

        let prepared: PreparedGeometries = Rc::new(
            rules
                .iter()
                .map(|rule| PreparedGeometry::from(rule.geometry.clone()))
                .collect(),
        );
        *cache.borrow_mut() = Some((ruleset_id, prepared.clone()));
        prepared
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::rule::RuleId;
    use geo::{Geometry, LineString, Polygon, Relate};

    fn rules() -> Vec<Rule> {
        vec![
            Rule {
                id: "zone-a".to_string(),
                properties: Default::default(),
                geometry: Geometry::Polygon(Polygon::new(
                    LineString::from(vec![
                        (0.0, 0.0),
                        (0.0, 1.0),
                        (1.0, 1.0),
                        (1.0, 0.0),
                        (0.0, 0.0),
                    ]),
                    vec![],
                )),
            },
            Rule {
                id: "zone-b".to_string(),
                properties: Default::default(),
                geometry: Geometry::Polygon(Polygon::new(
                    LineString::from(vec![
                        (10.0, 10.0),
                        (10.0, 11.0),
                        (11.0, 11.0),
                        (11.0, 10.0),
                        (10.0, 10.0),
                    ]),
                    vec![],
                )),
            },
        ]
    }

    #[test]
    fn same_ruleset_id_reuses_the_prepared_handle() {
        let rules = rules();
        let first = get_or_prepare(&rules, 100);
        let second = get_or_prepare(&rules, 100);

        assert!(Rc::ptr_eq(&first, &second));
    }

    #[test]
    fn changing_ruleset_id_replaces_the_prepared_handle() {
        let rules = rules();
        let first = get_or_prepare(&rules, 101);
        let second = get_or_prepare(&rules, 102);

        assert!(!Rc::ptr_eq(&first, &second));
    }

    #[test]
    fn prepared_handle_is_indexed_in_rule_order() {
        let prepared = get_or_prepare(&rules(), 103);

        assert_eq!(prepared.len(), 2);
        assert!(rules()[0].geometry.relate(&prepared[RuleId(0).0 as usize]).is_intersects());
        assert!(!rules()[0].geometry.relate(&prepared[RuleId(1).0 as usize]).is_intersects());
        assert!(rules()[1].geometry.relate(&prepared[RuleId(1).0 as usize]).is_intersects());
    }
}
