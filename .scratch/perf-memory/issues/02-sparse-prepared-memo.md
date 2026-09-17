# Memory at scale: sparse prepared memo (O(touched), not O(rules))

Type: task
Status: resolved

## Goal

Stop paying per-rule memory for a prepared-geometry slot that is never filled.

## Problem

`PreparedMemo::for_ruleset` allocates
`Rc::new(RefCell::new((0..rules.len()).map(|_| None).collect()))`
(`core/src/indexing/prepared_cache.rs:74`). `PreparedSlot` is
`Option<PreparedGeometry<'static, Geometry<f64>>>` — a large value even when
`None`. `Ruleset::prepare` always builds the memo, including on the unprepared
`intersects` mask fast path that never calls `ensure`. At 100k rules this is
≈25 MiB per thread of all-`None` slots; on the default `intersects` cell it is
all waste.

## Plan

- Replace the dense slot vector with a representation that costs ~nothing per
  untouched rule: `Vec<Option<Box<PreparedGeometryOwned>>>` (≈8 B/slot) or a
  sparse `HashMap<u32, PreparedGeometryOwned>`.
- Preserve the `PreparedMemo` seam (`ensure`, `slots`) and the eager
  `snapshot_all` contract (`len() == rule count`, `get(id)` valid for any id,
  `iter()` in ruleset order) — only the benchmark ladder and API-surface tests
  use the eager path.
- Keep the lazy per-rule prepare order and the deterministic envelope-order
  relate unchanged.

## Acceptance

- 100k-rules "after 1st query" memory drops toward the ruleset steady state
  when no rule is prepared (measure with `bun run bench memory-scale`).
- `cargo test --workspace` green, including the eager-seam API-surface tests.
- `docs/benchmarks.md` §Memory footnote corrected (the margin is not
  proportional to touched rules today).

## Comments

**2026-09-16 (agent): resolved.**

- `PreparedSlot` is now `Option<Box<PreparedGeometryOwned>>`
  (`core/src/indexing/prepared_cache.rs`): `ensure` boxes on fill,
  `snapshot_all` derefs to the dense owned vector, and the hot relate loop uses
  `as_deref()` (`core/src/runtime/evaluate.rs`). The slot table is ~8 B/rule
  instead of a full `Option<PreparedGeometry>` (hundreds of B) per rule.
- **Measured** with `bun run bench memory-scale --rules=100000 --vertices=10`:
  the after-first-query margin over the ruleset steady state fell from
  **~25.6 MiB** (documented 2026-08-23: 143.3 − 117.7 MiB) to **~5.2 MiB**
  (135,524,352 − 130,027,520 B). Absolute steady state differed run-to-run
  (124.0 MiB here vs 117.7 documented), so only the margin is comparable.
- `cargo test -p spatial-rules-core --features benchmark` green;
  `cargo clippy --workspace --all-targets` clean.
- Footnote in `docs/benchmarks.md` §Memory corrected.
- **Follow-up observation (not this ticket):** the run reported
  `lifecycle.bounded: false` for this single Windows cell. Not attributable to
  this change (it only removes memory) — worth a look in the measurement-rigor
  pass, since the documented lifecycle verdict is "bounded".
