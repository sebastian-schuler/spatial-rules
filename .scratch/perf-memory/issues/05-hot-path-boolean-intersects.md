# Hot path: don't prepare for a boolean `intersects`

Type: task
Status: resolved

## Goal

Stop populating the prepared memo when only a boolean `intersects` answer is
needed, and stop recomputing per-candidate constants per rule.

## Problem

`evaluate_mask_intersects` (`core/src/runtime/evaluate.rs:329`) already uses the
unprepared `geo::Intersects` fast path with early exit, but the resolve mask
(`evaluate_resolve_mask`, `evaluate.rs:630`) and the rich `query` boolean
admission (`admit_by_predicate`, `evaluate.rs:527`) route `intersects` through
`relate_touched` → `memo.ensure` → `relate`, building full DE-9IM matrices and
preparing every touched rule. On intersects-only workloads this is pure
overhead and grows the per-thread memo.

## Plan

- For `intersects` where only a boolean is needed (resolve mask; rich boolean
  admission without overlap), use the boolean predicate and skip preparation.
  Keep the full-matrix path where the matrix is actually consumed (the six
  directional predicates, overlap, explanation).
- Hoist the candidate's geodesic area out of `overlap_metric`
  (`core/src/runtime/evaluate.rs:76`): it is constant per candidate but computed
  once per matched rule (`:410`).

## Acceptance

- `intersects`-only resolve/rich batches no longer grow the memo; byte-identical
  results.
- `batch_resolve/*` and the mask baseline hold or improve; `cargo test` green.
- Measure with `bun run bench samples` (perf-memory 01) — criterion's
  change-vs-baseline line is not evidence; judge the delta against the spread.

## Comments

**2026-09-16 (agent): resolved — ~2.4× on the resolve/rich path.**

What landed (`core/src/runtime/evaluate.rs`):

- `relate_touched` became `for_each_admitted`: it takes `FnMut(RuleId)` and
  itself decides the predicate. For `SpatialPredicate::Intersects` it uses
  geo's boolean `Intersects` on the unprepared rules and **never calls
  `memo.ensure`**; for the six directional predicates it keeps the prepared
  relate + `spatial_predicate_holds`. The `&IntersectionMatrix` is gone from the
  interface — no caller ever needed the raw matrix.
- `overlap_metric` now takes the candidate's geodesic area, computed once per
  candidate in `evaluate_result` (`candidate_area`) instead of once per matched
  rule.

**Controlled A/B** (`bun run bench samples --filter=batch_resolve`, 5 runs each,
same session; the "before" side restored the prepared path with a temporary
`false &&` guard):

| bench | prepared relate | boolean intersects | delta | noise |
|---|---|---|---|---|
| `mask_baseline` (control) | 3.303 ms | 3.326 ms | +0.7% | ±3.0% |
| `resolve_mask` | 10.805 ms | **4.576 ms** | **−57.6%** | ±7.5% |
| `resolve_full` | 11.423 ms | **4.785 ms** | **−58.1%** | ±3.0% |

The unchanged mask path is the control and stayed within noise, which is what
makes the resolve delta trustworthy. The mechanism: a full 9-cell matrix per
touched rule loses to a boolean predicate that short-circuits, even against a
*warm prepared* memo.

A **methodology trap worth remembering**: an earlier criterion run reported this
change as "−0.8%, not significant" because a *prior post-05 run had already
overwritten* the `target/criterion` baseline with the new values. Stored
baselines are only as good as the last run that touched that bench; the
`--save`/`--compare` JSON is the reliable record.

**Correctness.** `intersects` on unprepared geometries is the established mask
path (ADR-0008, architecture-hardening 08) — byte-identical to the full-matrix
answer. Three existing memo tests used `Intersects` to exercise lazy
preparation; they now use `Contains` (a directional predicate) with two fixture
geometries enlarged so each still matches its intended rules, and a new test
`intersects_does_not_prepare_rules` pins the new behavior for both `query()` and
`resolve_mask()`.

`docs/benchmarks.md` §Surfaces gained a dated amendment with the A/B table; the
old "+3% / free" reading is marked superseded.

**Unmeasured:** the overlap-area hoist (no overlap benchmark exists) — it is
rationale-verified only. `cargo test --workspace --exclude
spatial-rules-python` + clippy green.
