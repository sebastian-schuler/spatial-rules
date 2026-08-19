# 08 — Cross-check the engine's predicate mapping against turf

Type: task
Status: resolved
Blocked by: None — can start immediately

Origin: 2026-08-19 architecture review, correctness gap (candidate 3 exploration).

## What to build

"Cross-check green" should certify the engine's predicate semantics, not just the geometry library's. Today the turf cross-check calls geo's DE-9IM matrix helpers (`is_intersects`, `is_coveredby`, …) directly — it verifies geo against turf, but never touches the engine's `SpatialPredicate → DE-9IM` mapping, so a regression in the engine's predicate wiring (the core of ADR-0008/0012) passes the cross-check. In particular the engine hand-rolls `covered_by` (`covered_by_holds`, four DE-9IM patterns) with a note that geo lacks a helper — yet the cross-check already calls `matrix.is_coveredby()`. Reconcile the two and make the cross-check exercise the engine's actual predicate evaluation (e.g. single-candidate queries through the ruleset, or a public predicate-holds accessor), diffed against the turf oracle for all seven predicates.

## Acceptance criteria

- [ ] The cross-check exercises the engine's predicate mapping (not raw geo matrix helpers) for all seven predicates, diffed against turf
- [ ] `covered_by` reconciled: either the engine uses geo's helper (hand-rolled logic deleted) or the hand-rolled patterns are verified against turf as part of the cross-check
- [ ] A deliberate regression in the engine's predicate mapping makes the cross-check fail (demonstrated)
- [ ] Existing fixtures and the turf oracle remain green; no behavior change outside the predicate evaluation path

## Answer

Implemented. `benchmarks/src/bin/cross_check.rs` now drives the engine — a
single-rule ruleset queried once per predicate via `query_mask` — so the
cross-check certifies the engine's `SpatialPredicate → DE-9IM` mapping, not
geo's raw matrix helpers. `covered_by` was reconciled by deleting the
hand-rolled `covered_by_holds` and using geo's `matrix.is_coveredby()` (the same
helper the old cross-check already verified against turf). Cross-check green; a
mapping regression now changes the emitted verdict and fails the diff.
