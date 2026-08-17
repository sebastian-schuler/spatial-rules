# Spatial Rules Engine v1 — Wayfinder Map

## Destination

Ship the spatial rules engine to the Definition of Done in `docs/Initial-plan.md` §43: a `SpatialRuleset` queryable from Node/Bun with a spatial predicate, property `where`, and `excludeRuleIds`, evaluating ~30 complex VRA-like rules against ~1,000 candidates per request, with dynamic atomic ruleset replacement, bounded container memory, Bun compatibility, and benchmark evidence against the JavaScript baseline. Execution is carried on this map — it closes only when §43 holds.

## Notes

- Domain and requirements: `docs/Initial-plan.md` is the source of truth. It stays a draft: locked decisions are recorded as **ADRs in `docs/adr/`**, not by editing the plan.
- Destination override: plan-don't-do is off for this effort — build phases become `task` tickets here; the map ends at §43, not at handoff.
- Skills every session should consult: grilling, domain-modeling, prototype; tdd and codebase-design for build tasks; research for AFK fact-finding.
- Benchmark-driven decisions: sync/async (§28) waits on the harness; algorithm ladder A–F (§32). Correctness reference: existing JavaScript implementation as baseline A, plus turf.js cross-check in tests (§33).
- Repo is not a git repo: research findings land as markdown files under `research/` here (no throwaway branches), linked from their tickets.
- HITL: user is available for grilling and prototype sessions.
- Standing preferences: correctness before micro-optimization (§41.8); batch-first (§41.1); minimize JS↔Rust crossings (§41.5).

## Decisions so far

<!-- the index — one line per closed ticket: enough to judge relevance, then zoom the link for the detail the ticket holds -->

- [Supported Node.js and Bun runtime matrix](issues/09-supported-runtimes.md) — Node 22/24/26 + Bun 1.3.14 (best-effort); target Node-API 8; findings in `research/09-supported-runtimes.md`.
- [Geometry stack: library, parser, representation, normalization](issues/01-geometry-stack.md) — pure-Rust `geo` 0.33 + `geojson`; `geo_types` internals; validate-and-reject at compile; workspace `core`/`node`/`benchmarks` (ADR-0001).
- [Spatial index choice for small static rulesets](issues/02-spatial-index.md) — packed `rstar` R*-tree (bulk-load) behind a `SpatialIndex` trait; per-candidate envelope lookup; scan kept as benchmark baseline; ladder sweep 30→100→1,000 decides (ADR-0002).
- [Prepared-geometry options in the chosen stack](issues/03-prepared-geometries.md) — prepare `PreparedGeometry` per worker (released geo is `!Send`), relate one-sided; revisit at geo 0.34 (`Send` fix merged); ladder E/F decides adoption.
- [Property query AST, typed storage, and property indexes](issues/04-property-query-ast.md) — typed `PropertyValue`; Mongo-style `where`; missing/mismatch = non-match; equality+`$in` indexes at compile; fixed spatial→property→exact order (ADR-0003).
- [Result representation and compact mask formats](issues/05-result-representation.md) — `Vec<CandidateOutcome>` aligned to input; `Uint8Array` mask (0/1/2) hot path; rich per-candidate API with string rule IDs (ADR-0004).
- [Invalid candidate handling and error model](issues/06-invalid-candidates-errors.md) — strict reject on invalid rules; candidate-level `invalid` in results; `SR_*` codes via `SpatialError`; Node throws `SpatialRulesError` with `.code` (ADR-0005).
- [Node binding stack and native binary packaging](issues/07-node-binding-stack.md) — napi-rs (`napi8`); per-platform optionalDependencies packages (linux x64/arm64 gnu+musl + win32-x64-msvc); Buffer-in/mask-out hot path; Bun smoke test non-blocking (ADR-0006).
- [Ruleset build cancellation and replacement progress](issues/10-ruleset-build-cancellation.md) — no cancellation/progress in v1; atomic `Arc` swap; observability `lastSwapTime`/`buildDurationMs`/active id (ADR-0007).
- [Predicate semantics and turf cross-check matrix](issues/11-predicate-semantics.md) — DE-9IM authoritative (`intersects`≠`FF*FF****`; `contains` `T*F**F***`; `within` `T**F*F***`); turf pinned as JTS-faithful oracle; disagreements investigated; predicates assume valid inputs (ADR-0008).
- [Sync vs async query and replacement API](issues/08-sync-async-api.md) — sync-first `query()`/`replace()`; `replace()` returns ADR-0007 observability; add `queryAsync()` only if harness p95 > 50 ms on the 1,000-candidate workload (ADR-0009).

## Not yet specified

<!-- see "Fog of war": in-scope fog you can't ticket yet; graduates as the frontier advances -->

## Out of scope

- GeoParquet ingestion, in core or as a separate package (§42.16) — beyond the §43 destination; future work per §39.
- Python bindings and Rust CLI — future work per §39.
- SQL-like query language and richer boolean expressions — out of scope for v1 per §11; future §39.
- Additional spatial predicates (covers, touches, overlaps, distance) and quantitative overlap area/ratio — not mandatory for the first release per §13–§14.
- Persisted compiled rulesets — future work per §39.
- Everything in §3 non-goals (PostGIS replacement, distributed DB, spatial microservice, nearest-neighbor, routing, raster/GDAL, SQL DB, VRA-specific framework).
