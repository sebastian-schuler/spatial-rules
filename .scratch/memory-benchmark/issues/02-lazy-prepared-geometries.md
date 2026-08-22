# 02 — Lazy per-rule prepared geometries (serving-memory follow-up)

Type: task
Status: resolved
Blocked by: None — can start immediately

Origin: follow-up to memory-benchmark ticket 01. The scaling benchmark showed
the serving footprint is dominated by the per-thread prepared-geometry cache
(ADR-0010): the first query prepares **every** rule regardless of whether any
candidate touches it, so a 100k-rule × 100-vertex ruleset holds ~1.5 GiB of
prepared geometries (~15 kB/rule) on top of a ~260 MiB ruleset — and turf
holding the same data is only ~640 MiB. `docs/benchmarks.md` §Memory "Engine vs
turf footprint" records the gap.

## What to build

Make the **internal query path** prepare rule geometries **lazily, per rule,
on first touch** (memoized per thread, keyed by ruleset id), so serving memory
becomes proportional to the rules candidates actually relate against. The
public `prepared()` seam stays **eager** (see Design). No behavior change to
query results or the API.

## Design

- **Two paths split.** `Ruleset::prepare`/`query`/`query_mask` (the hot path)
  go lazy. `Ruleset::prepared()` (`PreparedRuleGeometries`, `core/src/ruleset.rs`)
  keeps its dense contract — `len() == rule count`, `get(id)` valid for any id,
  `iter()` in ruleset order — and forces full prepare when called (the
  benchmark ladder's prepare rung and `api_surface.rs` depend on it).
- **Per-thread memo.** Replace the dense `Vec<PreparedGeometry>` in the
  `thread_local!` cache (`core/src/prepared_cache.rs`) with a per-rule memo
  (`Vec<Option<…>>` or a `HashMap<RuleId, …>`), still keyed by the ruleset's
  atomic id and invalidated wholesale on `replace` (same ruleset-id switch as
  today).
- **Keep the relate loop dense.** Prefer a **batch-level** design: after the
  envelope-filter pass collects the touched rule ids, prepare exactly the
  missing ones once, then run the relate over a dense structure — so the hot
  loop is not littered with per-candidate `is_some()` branches. (A per-rule
  check inside the loop is the fallback; benchmark the delta — expect <0.1%.)
- **geo 0.34 complementarity.** This removes the "prepare everything" bloat but
  not the per-thread duplication; the `Rc → Arc` sharing (post-v1 ticket 05)
  is orthogonal and stays separate.
- **Public API surface unchanged.** `query`, `queryAsync`, `replace`,
  `prepared()` signatures and semantics unchanged; `api_surface.rs` stays green
  as-is.

## Acceptance criteria

- [x] Serving memory proportional to touched rules: at the 100k×100 memory
      cell with 1,000 candidates (which touch ~1,000 of 100k rules) the
      post-query resident footprint drops from ~1.8 GiB toward ruleset + ~15 MB,
      verified via `bun run bench memory-scale` and `bun run bench memory-turf`
- [x] Steady-state throughput unchanged: warmed batches in the memory-scale /
      perf / scale harnesses report the same `queries_per_sec` (cold batch
      excluded) as before
- [x] Cold first batch not slower than today (expected: faster — no 100k-rule
      prepare spike)
- [x] Worst case unchanged: a workload whose candidates touch every rule
      prepares everything, same memory and speed as today (covered by a test
      that touches all rules)
- [x] Behavior-identical results: existing core tests (`query.rs`, `engine.rs`,
      `complex.rs`, `api_surface.rs`, proptest) green, including the eager
      `prepared()` seam tests
- [x] A correctness test pins the lazy semantics: a query touching a subset of
      rules only prepares that subset (observable via a counter or by checking
      the touched rules are prepared)
- [x] Docs updated: `docs/benchmarks.md` §Memory serving-footprint table and
      the "Engine vs turf footprint" comparison re-recorded, README memory
      bullet refreshed, ADR-0010 amended, results re-verified on a clean
      `bun run bench memory-scale` run

## Notes

- The win is conditional on the candidate workload: sparse-touch → large
  savings; touch-everything → no change. Document that serving memory is now
  workload-dependent, and that worst-case capacity planning still sizes to the
  today ceiling.
- For the documented production shape (30 rules) this is a no-op — the payoff
  shows for large rulesets served sparsely (e.g. national zoning behind an
  HTTP endpoint: no first-request event-loop stall, memory proportional to
  query coverage).
- One-time prepare cost lands in the latency tail of the request that first
  touches a rule; amortized, and re-warmed per thread after each `replace`.

## Answer

Implemented (core change + benchmark re-verification + docs, no behavior change
to query results or the public API).

- **Design (per ticket).** Two paths split. The hot path
  (`Ruleset::prepare`/`query`/`query_mask`) is **lazy**: the `thread_local!`
  cache in `core/src/prepared_cache.rs` became a **per-rule memo**
  (`Vec<Option<PreparedGeometry>>` behind `Rc<RefCell<…>>`), still keyed by the
  ruleset's atomic id and invalidated wholesale on `replace`. The relate loop
  (`core/src/evaluate.rs`) checks each touched rule's slot, defers the
  first-touch unprepared ones, and prepares exactly those — so serving memory
  is proportional to touched rules. The public eager seam `Ruleset::prepared()`
  (`PreparedRuleGeometries`) keeps its dense contract (`len() == rule count`,
  `get(id)` valid for any id, `iter()` in ruleset order) by force-preparing
  every slot and snapshotting a dense `Vec` (PreparedGeometry is `Clone` in
  geo 0.33); `api_surface.rs` and the ladder stay green unchanged.
- **Batch pre-pass rejected by measurement.** A batch-level design that
  collects the touched union up front and prepares once was implemented first;
  it ran the envelope filter a second time per candidate and regressed the
  sparse-touch `queries_per_sec` cell by ~30% (that workload is
  index-traversal bound, relate is negligible). Reverted to the ticket's
  sanctioned per-rule fallback, which measures at-or-better than the old eager
  cache.
- **Result ordering kept deterministic.** The relate loop collects each
  candidate's envelope-filtered rule ids in index order, prepares exactly the
  missing ones, then relates in that order — so `Matched.rule_ids` keeps the
  eager path's deterministic ascending order whether or not the memo was
  already warm. Pinned by `rule_ids_stay_in_envelope_order_with_a_partially_warm_memo`.
- **The memo is a deep module.** `prepared_cache.rs` exposes one
  `PreparedMemo` seam bundling ruleset identity, the rule slice, and the shared
  slots — `Ruleset` and the relate loop never see the raw storage (removes the
  `(slots, rules)` clump and the leaked `rules_slice()` accessor that an early
  review flagged).
- **New tests.** `prepared_cache.rs`: subset-only preparation, id-switch
  wholesale reset, dense-vs-lazy relate equality, `prepare_all` order.
  `ruleset.rs`: query touching a subset prepares only that subset; touch-all
  query prepares every rule (worst case); eager seam force-prepares without
  any query.
- **Verified results** (release, Windows, default grid; full suite green
  including proptest and `api_surface`): serving after the first query at
  1,000 candidates dropped from 359 MiB → 143 MiB (100k×10), 1.78 GiB →
  286 MiB (100k×100); the cold first batch at 100k×100 dropped 1,875 ms →
  ~2 ms; warm `queries_per_sec` unchanged within run variance (7.1 M vs
  7.1 M at 100k×100). The engine's serving footprint now **beats turf at every
  cell** (100k×100: 282 MiB vs turf's 641 MiB). Lifecycle unchanged: no
  per-replacement leak, same stepwise-with-drops Windows allocator warmup
  (`bounded: false` cells unchanged).
- **Docs:** `docs/benchmarks.md` §Memory scaling table + findings + turf
  comparison re-recorded (2026-08-23), ADR-0010 amended (lazy addendum),
  README memory bullet refreshed, roadmap P0 updated.