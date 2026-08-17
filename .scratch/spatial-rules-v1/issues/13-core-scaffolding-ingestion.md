# Rust core: scaffolding, types, GeoJSON ingestion, validation

Type: task
Status: resolved

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

## Answer

Built the `spatial-rules-core` foundation (TDD at the pre-agreed seams below), committed to `main`.

**Workspace** (`Cargo.toml` root, `resolver = "2"`): `core/` (crate `spatial-rules-core`), `node/` (`spatial-rules-node`, empty `cdylib` stub), `benchmarks/` (`spatial-rules-benchmarks`, empty bin stub). Deps pinned via `[workspace.dependencies]`: `geo 0.33`, `geo-types 0.7`, `geojson 1.0`, `serde_json 1`.

**Types** (`core/src/{rule,candidate,properties}.rs`):
- `Rule { id, properties, geometry }` — string id, `BTreeMap<String, PropertyValue>`, `Geometry<f64>`.
- `Candidate { id, geometry }` — candidate properties are not used by the engine in v1.
- `PropertyValue::{Null,Bool,Int(i64),Float(f64),Str}`; JSON numbers → `Int` when integral (by value, so `10.0` → `Int(10)`), else `Float`; nested objects/arrays are skipped (out of v1 scope).
- `RuleId(u32)` newtype for the `0..n-1` mapping (assigned at ruleset build in ticket 14).

**Errors** (`core/src/error.rs`): `SpatialError { code, message }` + `ErrorCode` with the full stable `SR_*` set from ADR-0005.

**Ingestion** (`core/src/ingestion.rs`): `parse_geojson` (permissive geometry, strict on malformed JSON → `SR_INVALID_GEOJSON`); `feature_geometry` (`geojson` → `geo_types::Geometry<f64>` via fallible `TryFrom`); `rule_from_feature` / `candidate_from_feature`; `rules_from_geojson` / `candidates_from_geojson`. Feature id comes from `id` or `properties.id`; missing → `SR_INVALID_GEOJSON`.

**Validation** (`core/src/validation.rs`): `ensure_supported_geometry` (Polygon/MultiPolygon only → `SR_UNSUPPORTED_GEOMETRY_TYPE`); `validate_rule_geometry` (supported type + `geo::Validation::is_valid()` → `SR_INVALID_GEOMETRY`; strict reject per ADR-0005).

**Tests**: 24 green (`cargo test --workspace`), 6 unit + 18 integration; clippy clean. Seams tested: error-code strings; property typing (integral/float/nested-skip); parse/malformed; polygon & multipolygon round-trip; id fallback; missing id/geometry; hole-contained valid vs hole-outside invalid; self-intersection (bowtie) invalid; NaN invalid; unsupported Point. No turf cross-check here — predicate semantics is ticket 15's concern.

Run: `cargo test --workspace` / `cargo clippy --workspace --all-targets`.

