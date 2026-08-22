//! Thread-local memo of per-rule prepared geometries (ADR-0010).
//!
//! The query path prepares each rule's geometry **lazily, on first touch**
//! (memory-benchmark ticket 02), so serving memory is proportional to the
//! rules candidates actually relate against — not the whole ruleset. The memo
//! stays keyed by the ruleset's atomic id and is invalidated wholesale when it
//! changes (`replace`, ADR-0007). The public eager seam
//! (`Ruleset::prepared`) force-fills every slot and snapshots a dense handle.

use std::cell::RefCell;
use std::rc::Rc;
use std::sync::atomic::{AtomicU64, Ordering};

use geo::{Geometry, PreparedGeometry};

use crate::rule::{Rule, RuleId};

type PreparedGeometryOwned = PreparedGeometry<'static, Geometry<f64>>;

/// One rule's slot in the per-thread memo: unprepared until first touched.
pub(crate) type PreparedSlot = Option<PreparedGeometryOwned>;

/// Per-rule memo for one ruleset, shared per thread. Interior mutability lets
/// later batches fill slots while earlier handles stay alive.
pub(crate) type PreparedSlots = Rc<RefCell<Vec<PreparedSlot>>>;

/// Dense fully-prepared snapshot (the eager seam's contract): every rule
/// prepared, indexed by rule position.
pub(crate) type PreparedGeometries = Rc<Vec<PreparedGeometryOwned>>;

/// Assigns each `Ruleset` a unique identity, used as the per-thread cache key.
static NEXT_RULESET_ID: AtomicU64 = AtomicU64::new(1);

thread_local! {
    static PREPARED_CACHE: RefCell<Option<(u64, PreparedSlots)>> = const { RefCell::new(None) };
}

pub(crate) fn next_ruleset_id() -> u64 {
    NEXT_RULESET_ID.fetch_add(1, Ordering::Relaxed)
}

fn prepare_rule(rule: &Rule) -> PreparedGeometryOwned {
    PreparedGeometry::from(rule.geometry.clone())
}

/// Return the per-thread memo for `ruleset_id`, replacing a stale entry when
/// the ruleset changed (wholesale invalidation on replace).
pub(crate) fn lazy_slots(rules: &[Rule], ruleset_id: u64) -> PreparedSlots {
    PREPARED_CACHE.with(|cache| {
        let mut cache = cache.borrow_mut();
        if let Some((id, slots)) = cache.as_ref() {
            if *id == ruleset_id {
                return slots.clone();
            }
        }
        let slots: PreparedSlots =
            Rc::new(RefCell::new((0..rules.len()).map(|_| None).collect()));
        *cache = Some((ruleset_id, slots.clone()));
        slots
    })
}

/// Prepare exactly the rules in `rule_ids` that have no slot yet, leaving
/// untouched rules unprepared. `slots` must come from [`lazy_slots`] for the
/// same `rules`.
pub(crate) fn ensure_prepared(slots: &PreparedSlots, rules: &[Rule], rule_ids: &[RuleId]) {
    let mut slots = slots.borrow_mut();
    for &rule_id in rule_ids {
        let index = rule_id.index();
        debug_assert!(index < rules.len(), "rule id out of range");
        if index < rules.len() && slots[index].is_none() {
            slots[index] = Some(prepare_rule(&rules[index]));
        }
    }
}

/// Force-prepare **every** rule and return a dense snapshot in ruleset order —
/// the eager seam's contract (`len() == rule count`, `get(id)` valid for any
/// id). Called by `Ruleset::prepared`, never by the query path.
pub(crate) fn prepare_all(rules: &[Rule], ruleset_id: u64) -> PreparedGeometries {
    let all: Vec<RuleId> = (0..rules.len()).map(|index| RuleId(index as u32)).collect();
    let slots = lazy_slots(rules, ruleset_id);
    ensure_prepared(&slots, rules, &all);
    let dense: PreparedGeometries = Rc::new(
        slots
            .borrow()
            .iter()
            .map(|slot| slot.as_ref().expect("prepare_all fills every slot").clone())
            .collect(),
    );
    dense
}

#[cfg(test)]
pub(crate) fn slot_is_prepared(ruleset_id: u64, index: usize) -> bool {
    PREPARED_CACHE.with(|cache| {
        cache
            .borrow()
            .as_ref()
            .filter(|(id, _)| *id == ruleset_id)
            .map(|(_, slots)| slots.borrow()[index].is_some())
            .unwrap_or(false)
    })
}

#[cfg(test)]
mod tests {
    use super::*;
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
    fn lazy_slots_start_empty_and_are_reused_for_the_same_ruleset() {
        let rules = rules();
        let first = lazy_slots(&rules, 100);
        assert!(first.borrow().iter().all(Option::is_none));

        let second = lazy_slots(&rules, 100);
        assert!(Rc::ptr_eq(&first, &second));
    }

    #[test]
    fn changing_ruleset_id_resets_the_memo_wholesale() {
        let rules = rules();
        ensure_prepared(&lazy_slots(&rules, 101), &rules, &[RuleId(0)]);
        assert!(slot_is_prepared(101, 0));

        let fresh = lazy_slots(&rules, 102);
        // The new memo starts empty even though rule 0 was prepared before.
        assert!(fresh.borrow().iter().all(Option::is_none));
    }

    #[test]
    fn ensure_prepared_prepares_only_the_requested_subset() {
        let rules = rules();
        let slots = lazy_slots(&rules, 103);

        ensure_prepared(&slots, &rules, &[RuleId(0)]);
        assert!(slot_is_prepared(103, 0));
        assert!(!slot_is_prepared(103, 1));

        // First touch of the remaining rule fills only that slot.
        ensure_prepared(&slots, &rules, &[RuleId(1)]);
        assert!(slot_is_prepared(103, 1));
    }

    #[test]
    fn prepared_relates_identically_to_a_fresh_prepare() {
        let rules = rules();
        let slots = lazy_slots(&rules, 104);
        ensure_prepared(&slots, &rules, &[RuleId(0)]);

        let candidate = Geometry::Polygon(Polygon::new(
            LineString::from(vec![
                (0.5, 0.5),
                (0.5, 1.5),
                (1.5, 1.5),
                (1.5, 0.5),
                (0.5, 0.5),
            ]),
            vec![],
        ));
        let borrowed = slots.borrow();
        let lazy_matrix = candidate.relate(borrowed[0].as_ref().unwrap());
        drop(borrowed);

        let dense = prepare_all(&rules, 105);
        let eager_matrix = candidate.relate(&dense[0]);

        assert_eq!(lazy_matrix, eager_matrix);
    }

    #[test]
    fn prepare_all_fills_every_slot_in_rule_order() {
        let rules = rules();
        let dense = prepare_all(&rules, 106);

        assert_eq!(dense.len(), 2);
        assert!(rules[0].geometry.relate(&dense[RuleId(0).index()]).is_intersects());
        assert!(!rules[0].geometry.relate(&dense[RuleId(1).index()]).is_intersects());
        assert!(rules[1].geometry.relate(&dense[RuleId(1).index()]).is_intersects());
        assert!(slot_is_prepared(106, 0) && slot_is_prepared(106, 1));
    }
}
