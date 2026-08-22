//! Thread-local memo of per-rule prepared geometries (ADR-0010).
//!
//! The query path prepares each rule's geometry **lazily, on first touch**
//! (memory-benchmark ticket 02), so serving memory is proportional to the
//! rules candidates actually relate against — not the whole ruleset. The memo
//! stays keyed by the ruleset's atomic id and is invalidated wholesale when it
//! changes (`replace`, ADR-0007). The public eager seam
//! (`Ruleset::prepared`) force-fills every slot and snapshots a dense handle.
//!
//! [`PreparedMemo`] is the whole seam: it bundles the ruleset identity, the
//! rule slice it prepares from, and the shared slots — callers never see the
//! raw storage or a bare `&[Rule]` (the `(slots, rules)` clump that earlier
//! iterations leaked into `Ruleset`).

use std::cell::{Ref, RefCell};
use std::rc::Rc;
use std::sync::atomic::{AtomicU64, Ordering};

use geo::{Geometry, PreparedGeometry};

use crate::rule::{Rule, RuleId};

type PreparedGeometryOwned = PreparedGeometry<'static, Geometry<f64>>;

/// One rule's slot in the per-thread memo: unprepared until first touched.
pub(crate) type PreparedSlot = Option<PreparedGeometryOwned>;

/// The shared per-thread slots for one ruleset; interior mutability lets later
/// batches fill slots while earlier handles stay alive.
type PreparedSlots = Rc<RefCell<Vec<PreparedSlot>>>;

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

/// One ruleset's per-rule prepared-geometry memo on the current thread
/// (ADR-0010). Owns the keying (ruleset identity), the rule slice it prepares
/// from, and the shared slots; interior mutability lets later batches fill
/// slots while earlier handles stay alive.
pub(crate) struct PreparedMemo<'a> {
    rules: &'a [Rule],
    slots: Rc<RefCell<Vec<PreparedSlot>>>,
}

impl<'a> PreparedMemo<'a> {
    /// The per-thread memo for `ruleset_id`, replacing a stale entry when the
    /// ruleset changed (wholesale invalidation on replace).
    pub(crate) fn for_ruleset(rules: &'a [Rule], ruleset_id: u64) -> Self {
        let slots = PREPARED_CACHE.with(|cache| {
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
        });
        PreparedMemo { rules, slots }
    }

    /// Prepare exactly the rules in `rule_ids` that have no slot yet, leaving
    /// untouched rules unprepared. Callers pass ids in the order they will
    /// relate against; warm batches find everything prepared, so this is a
    /// no-op scan.
    pub(crate) fn ensure(&self, rule_ids: &[RuleId]) {
        let mut slots = self.slots.borrow_mut();
        for &rule_id in rule_ids {
            let index = rule_id.index();
            debug_assert!(index < self.rules.len(), "rule id out of range");
            if index < self.rules.len() && slots[index].is_none() {
                slots[index] = Some(prepare_rule(&self.rules[index]));
            }
        }
    }

    /// The shared slots, borrowed immutably for a relate pass. Every id passed
    /// to [`PreparedMemo::ensure`] is guaranteed present.
    pub(crate) fn slots(&self) -> Ref<'_, Vec<PreparedSlot>> {
        self.slots.borrow()
    }

    /// Force-prepare **every** rule and return a dense snapshot in ruleset
    /// order — the eager seam's contract (`len() == rule count`, `get(id)`
    /// valid for any id). Called by `Ruleset::prepared`, never by the query
    /// path.
    pub(crate) fn snapshot_all(&self) -> PreparedGeometries {
        let all: Vec<RuleId> = (0..self.rules.len()).map(|index| RuleId(index as u32)).collect();
        self.ensure(&all);
        Rc::new(
            self.slots()
                .iter()
                .map(|slot| slot.as_ref().expect("snapshot_all fills every slot").clone())
                .collect(),
        )
    }
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
    fn memo_starts_empty_and_is_reused_for_the_same_ruleset() {
        let rules = rules();
        let memo = PreparedMemo::for_ruleset(&rules, 100);
        assert!(memo.slots().iter().all(Option::is_none));

        let again = PreparedMemo::for_ruleset(&rules, 100);
        assert!(Rc::ptr_eq(&memo.slots, &again.slots));
    }

    #[test]
    fn changing_ruleset_id_resets_the_memo_wholesale() {
        let rules = rules();
        let memo = PreparedMemo::for_ruleset(&rules, 101);
        memo.ensure(&[RuleId(0)]);
        assert!(slot_is_prepared(101, 0));

        let fresh = PreparedMemo::for_ruleset(&rules, 102);
        // The new memo starts empty even though rule 0 was prepared before.
        assert!(fresh.slots().iter().all(Option::is_none));
    }

    #[test]
    fn ensure_prepares_only_the_requested_subset() {
        let rules = rules();
        let memo = PreparedMemo::for_ruleset(&rules, 103);

        memo.ensure(&[RuleId(0)]);
        assert!(slot_is_prepared(103, 0));
        assert!(!slot_is_prepared(103, 1));

        // First touch of the remaining rule fills only that slot.
        memo.ensure(&[RuleId(1)]);
        assert!(slot_is_prepared(103, 1));
    }

    #[test]
    fn prepared_relates_identically_to_a_fresh_prepare() {
        let rules = rules();
        let memo = PreparedMemo::for_ruleset(&rules, 104);
        memo.ensure(&[RuleId(0)]);

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
        let lazy_matrix = candidate.relate(memo.slots()[0].as_ref().unwrap());

        let dense = PreparedMemo::for_ruleset(&rules, 105).snapshot_all();
        let eager_matrix = candidate.relate(&dense[0]);

        assert_eq!(lazy_matrix, eager_matrix);
    }

    #[test]
    fn snapshot_all_fills_every_slot_in_rule_order() {
        let rules = rules();
        let dense = PreparedMemo::for_ruleset(&rules, 106).snapshot_all();

        assert_eq!(dense.len(), 2);
        assert!(rules[0].geometry.relate(&dense[RuleId(0).index()]).is_intersects());
        assert!(!rules[0].geometry.relate(&dense[RuleId(1).index()]).is_intersects());
        assert!(rules[1].geometry.relate(&dense[RuleId(1).index()]).is_intersects());
        assert!(slot_is_prepared(106, 0) && slot_is_prepared(106, 1));
    }
}