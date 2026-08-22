# 02 — Lazy per-rule prepared geometries (serving-memory follow-up)

Type: task
Status: ready-for-agent
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

- [ ] Serving memory proportional to touched rules: at the 100k×100 memory
      cell with 1,000 candidates (which touch ~1,000 of 100k rules) the
      post-query resident footprint drops from ~1.8 GiB toward ruleset + ~15 MB,
      verified via `bun run bench memory-scale` and `bun run bench memory-turf`
- [ ] Steady-state throughput unchanged: warmed batches in the memory-scale /
      perf / scale harnesses report the same `queries_per_sec` (cold batch
      excluded) as before
- [ ] Cold first batch not slower than today (expected: faster — no 100k-rule
      prepare spike)
- [ ] Worst case unchanged: a workload whose candidates touch every rule
      prepares everything, same memory and speed as today (covered by a test
      that touches all rules)
- [ ] Behavior-identical results: existing core tests (`query.rs`, `engine.rs`,
      `complex.rs`, `api_surface.rs`, proptest) green, including the eager
      `prepared()` seam tests
- [ ] A correctness test pins the lazy semantics: a query touching a subset of
      rules only prepares that subset (observable via a counter or by checking
      the touched rules are prepared)
- [ ] Docs updated: `docs/benchmarks.md` §Memory serving-footprint table and
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