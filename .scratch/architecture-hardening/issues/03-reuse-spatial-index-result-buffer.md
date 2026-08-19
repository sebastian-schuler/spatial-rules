# 03 — Reuse the spatial-index result buffer across the batch

Type: task
Status: resolved
Blocked by: 01 (deepen the candidate intake)

Origin: 2026-08-19 architecture review, candidate 6.

## What to build

A batch query should not allocate and sort a fresh result vector for every candidate. Today each candidate's envelope lookup returns an owned, sorted, deduplicated vector — so a 1,000-candidate batch allocates and sorts 1,000 vectors even though the engine only iterates each result. Deepen the spatial-index seam with a fill-a-buffer or iterate form (both the R-tree and linear-scan adapters satisfy it) so the per-candidate allocation moves out of the hot loop. Behavior must be identical: same matched rule ids, same order.

**Measure first.** The win is shape-dependent: with the current ~30 country-sized rules the index returns nearly everything, so the per-candidate allocation may be unmeasurable against relate cost. Quantify it in the ladder at current shapes before committing to the seam change; if it is nil, keep the change only if behavior-neutral and documented — the ladder is the arbiter either way.

## Acceptance criteria

- [ ] A batch query performs no per-candidate allocation for the spatial-index result; one reused buffer (or iterator) serves the whole batch
- [ ] Both index adapters satisfy the deepened seam; returned rule ids identical to today (same set, order, dedup)
- [ ] Measurement first: the ladder (and the load harness where relevant) quantifies the allocation cost at current shapes; the keep/cut decision is recorded against that measurement
- [ ] Hot-path behavior unchanged; core tests, node + Bun smoke, and the criterion ladder green

## Answer

Implemented (commit `4cae54a`). The `SpatialIndex` seam gained
`query_envelope_into(&self, envelope, &mut Vec<RuleId>)` (both R-tree and
linear-scan adapters), and `PreparedQuery` reuses one scratch buffer across the
batch instead of allocating a fresh vector per candidate. Measurement: at the
current ~30 country-scale rules the index returns nearly all rules, so the
per-candidate allocation is unmeasurable against relate cost (ladder D ≈ C ≈ B);
the change was kept because it is behavior-neutral and documented. Same matched
ids, order, and dedup.
