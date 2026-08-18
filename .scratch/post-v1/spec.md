# Post-v1 feature work — Spec

Four future-work items from `docs/Initial-plan.md` §39 plus one dependency upgrade, locked by the 2026-08-18 grilling session into ADRs 0011–0013. SQL-like query language is **removed** from the plan; `distance` predicates are **deferred**.

## Scope

1. **Richer JSON `where` operators** — `$not`, `$nin`, `$exists` (ADR-0011). Ticket 01.
2. **Spatial predicates** — `covers`, `covered_by`, `touches`, `overlaps` (ADR-0012). Ticket 02.
3. **Quantitative overlap** — `overlapArea` (m²) + `overlapRatio` ([0,1]), geodesic, rich path only (ADR-0012). Ticket 03.
4. **Persisted rulesets** — canonical JSON + recompile on load, deploy-time precompile (ADR-0013). Ticket 04.
5. **`geo` 0.34 upgrade** — opportunistic, when published (ADR-0010 follow-up). Ticket 05.
6. **Opt-in `queryAsync`** — off-main-thread query (libuv threadpool); sync `query()` stays the default. Ticket 06.
7. **Test comprehensiveness** — audit + expand the suite (proptest, error/edge matrix, fuzz, test-CI runtime matrix). Ticket 07.

## Sequencing

Richer operators → predicates → overlap → persistence, all on `geo` 0.33.1. The `geo` 0.34 bump is independent and gated on the upstream release (no git-dep pinning — keeps the crate publishable).

## Cross-cutting

- New ADRs 0011–0013; amendments to 0003/0008/0010.
- turf cross-check suite gains `@turf/boolean-covers` + `@turf/boolean-touches` (`boolean-overlap` already pinned).
- `serde` (derive) added to the workspace for persistence.
- Hot-path `Uint8Array` mask unchanged throughout.

## Open proposals (deferred, not ticketed)

Return-shape variants discussed 2026-08-19 — deferred pending a concrete consumer/response contract. Not ticketed; each is an additive napi method when needed.

- `filteredGeojson(candidates, query) -> String` — kept features as a GeoJSON string (pass-through `res.send`). **Most likely to be ticketed** if the endpoint returns the filtered FeatureCollection.
- `filteredFeatures(candidates, query) -> FeatureCollection` (JS objects) — convenience for consumers that transform the data.
- `queryRich` object variant — JS array of objects instead of a string.
- `keep`-indices helper — kept feature indices for cheap slicing.

Trigger: the real endpoint's response contract — if it returns the filtered GeoJSON, ticket `filteredGeojson`; if consumers need objects, ticket the object variant. Callers already hold the primitives (mask + buffered bytes; `queryRich` string), so these are ergonomics, not capability.

## Ticket index

- `issues/01-where-operators.md` — ready-for-agent
- `issues/02-spatial-predicates.md` — ready-for-agent
- `issues/03-overlap-area-ratio.md` — ready-for-agent (blocked by 02)
- `issues/04-canonical-rulesets.md` — ready-for-agent
- `issues/05-geo-034-upgrade.md` — needs-triage (gated on geo 0.34 release)
- `issues/06-query-async.md` — ready-for-agent (opt-in, off-main-thread)
- `issues/07-test-comprehensiveness.md` — ready-for-agent (broad; may split)
