# Rust core: scaffolding, types, GeoJSON ingestion, validation

Type: task
Status: open

## Context — read first

- `CONTEXT.md` (root) — glossary: candidate, rule, rule-id, ruleset, spatial predicate, property predicate, query, match. Use these terms.
- `docs/Initial-plan.md` — requirements source of truth; relevant here: §4 (data model), §10 (property types), §23 (GeoJSON boundary), §33 (correctness cases), §34 (invalid geometry).
- ADRs in `docs/adr/` that bind this ticket: **0001** (geo 0.33 + geojson, geo-types internals, validate-and-reject, workspace core/node/benchmarks), **0003** (`PropertyValue` typed storage), **0004** (result/outcome model), **0005** (strict reject on invalid rules; `SR_*` codes). Skim 0002/0008 for context.
- `research/01-geometry-stack.md` — crate facts: `geo` 0.33.1, `geo-types` 0.7.20, `geojson` 1.0.0 (MSRV 1.88 workspace). Parse is permissive; validity is a separate `geo::algorithm::Validation` gate.
- Skills: **tdd** (red→green, test at seams; agree the seams first) and **codebase-design** if the module seams are in question.
- Tracker: `docs/agents/issue-tracker.md` — claim by setting `Status: claimed`; resolve by `## Answer` + `Status: resolved` + map Decisions-so-far line.

## Question

Build the foundation of `spatial-rules-core` (execution on the map — destination override):

- Cargo workspace with `core/` (crate `spatial-rules-core`), `node/`, `benchmarks/` (ADR-0001). The `node/` and `benchmarks/` crates may be empty stubs here.
- `Rule`, `Candidate`, `PropertyValue` types (ADR-0003, ADR-0004); numeric `RuleId` mapping (`0..n-1`).
- `geojson` → `geo_types::Geometry<f64>` ingestion (permissive parse; validity is separate).
- `geo::Validation::is_valid()` gate for rule geometries; strict reject per ADR-0005.
- Workspace compiles; ingestion + validation unit tests green; tests where sensible cross-checked against turf.js.
