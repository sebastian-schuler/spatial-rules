# P2 — Realistic rules — Spec

The engine evolves from matcher to spatial policy engine (roadmap §Direction). P2 lands its two features — **temporal conditions** and **distance predicates** — behind the roadmap gate that distance semantics are decided and documented first. The 2026-08-23 grilling session settled the design into ADR-0016 (distance) and ADR-0017 (temporal).

## Scope

1. **CRS/geodesic + distance-semantics ADR** — spherical great-circle (Haversine) meters, `withinDistance` minimum-distance semantics, bounding-circle pre-filter, strict validation. Ticket 01 (resolved, ADR-0016).
2. **Temporal conditions** — query `at` + whole-clause `$activeAt` over scalar rule window properties. Ticket 02.
3. **`withinDistance` predicate** — query shape, distance admission, resolution integration. Ticket 03.
4. **P2 test suite** — haversine invariants, determinism, index-kind parity, temporal edge cases. Ticket 04.

## Sequencing

ADR (01) → withinDistance (03); temporal (02) is independent and parallel. Blocking: 03 blocked by 01; 04 by 02+03.

## Cross-cutting

- ADR-0016 and ADR-0017 are authoritative; tickets cite them.
- The engine stays pure and deterministic: the query supplies time (`at`); no wall clock in the query path.
- Distance is never planar (Initial-plan §14); antimeridian-safe by construction.
- `query()` and the match mask are unchanged; resolution (P1) extends additively.

## Explicitly deferred (additive later, no shape change)

- Ellipsoidal Karney geodesic (higher accuracy).
- `nearest` (documented Non-Goal, Initial-plan §72) and proximity bands (compose as repeated `withinDistance`).
- Temporal indexing; holidays; timezone offsets; sub-hour precision.
- Rhumb (loxodrome) distance.

## Ticket index

- `issues/01-distance-adr.md` — resolved
- `issues/02-temporal-conditions.md` — resolved
- `issues/03-within-distance.md` — resolved (blocked by 01)
- `issues/04-p2-tests.md` — resolved (blocked by 02, 03)