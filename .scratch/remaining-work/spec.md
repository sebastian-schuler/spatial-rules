# Remaining work — the post-P0/P1/P2/aggregation backlog

Status of everything left unticketed after the shipped tiers, so a fresh
session can pick up without re-deriving the state.

## Shipped (tickets resolved, committed)

- **P0** memory benchmark — `docs/benchmarks.md` §Memory.
- **P1** resolution — `.scratch/p1-resolution/` (priority, derived values, explanation, resolve API, property tests).
- **P2** realistic rules — `.scratch/p2-realistic-rules/` (temporal `$activeAt`, `withinDistance`).
- **Aggregation** — `.scratch/aggregation/` (per-candidate count/min/max/sum/avg/coverage).
- **Benchmark ladder additions** — `issues/01-benchmark-ladder.md` resolved (commit `d447f24`).
- **Integration server surfaces** — `issues/02-integration-server.md` resolved (commit `a012059`).
- **WASM + Python distribution** — decided by grilling; `issues/04-wasm-build.md` resolved, graduated to `.scratch/wasm/` with implementation tickets.
- **P3 PostgreSQL loader** — deferred, not rejected (`docs/roadmap.md` §P3).

## Open tickets in this directory

- `issues/03-streaming-geofencing.md` — ready-for-human (grilling)

## Roadmap fog, not yet ticketed (grill/triage when demand or scale proves out)

- **Rule composition / expression language** — now unblocked: the primitives
  (predicates, resolution, distance, temporal) exist after P1/P2; this is a
  compiler over them. The strongest unticketed *capability* candidate after
  streaming geofencing.
- Temporal indexing (time as a first-class indexed dimension) — demand-gated.
- Route-aware queries; H3/S2/geohash cells; compiled/mmap persisted format;
  CRS beyond the documented semantics; Postgres phases 2–5 — scale/positioning
  gated.

## Sequencing

- **01** and **02** shipped (committed); **04** decided and graduated to
  `.scratch/wasm/`.
- **03** needs a grilling/decision session with a human first; once the design
  is settled, it graduates to its own `.scratch/<feature>/` with implementation
  tickets.

## Working state

- Branch: `development`. All work committed; working tree clean.
- Docs: `docs/roadmap.md` (shipped tiers + deferred P3), ADRs 0001–0018,
  `CONTEXT.md`, `README.md`, `docs/examples.md` (verified end-to-end walkthrough).