# Geometry stack: library, parser, representation, normalization

Type: grilling
Status: resolved

## Question

Decide the geometry foundation of the Rust core (§42 items 1, 4, 5, 12; §4, §16, §23):

1. **Geometry library** — which crate(s) provide `Polygon`/`MultiPolygon`, the three predicates (intersects/contains/within), and eventual extras (covers, area)? Candidates: the `geo` ecosystem (georust), JTS ports, or something else. Constraints: WGS84 coordinates must not be naively treated as planar for area (§14), prepared/optimized reuse matters (§16), correctness before micro-optimization (§41.8).
2. **GeoJSON parser** — which crate parses the interchange format (§23), and what happens to the raw GeoJSON structures after parsing (compact internal representation, not retained object graphs).
3. **Internal representation** — compact native geometry storage, typed, minimal copies (§24).
4. **Normalization strategy** — what "prepare" means before index build (§16): validation, ring orientation, coordinate layout.
5. **Cargo workspace layout** — `core/`, `node/`, `benchmarks/` crate structure (§20).

Suggested session shape: dispatch a research subagent first for primary-source facts on candidate crates (geo, JTS ports, parser options), then grill the user on the decision with a recommendation. Locked decision becomes an ADR in `docs/adr/`.

## Answer

Locked (grilling session 2026-08-13):

- **Geometry library:** `geo` 0.33 (georust), pure Rust. Spatial predicates answered uniformly via `Relate` DE-9IM (`is_intersects` / `is_contains` / `is_within`), matching the `Intersects` / `Contains` / `Within` trait semantics.
- **GeoJSON parser:** `geojson` crate (`TryFrom` → `geo_types::Geometry<f64>`). Parsing is syntactic/permissive; validity is a separate explicit step.
- **Internal representation:** `geo_types::Geometry<f64>` directly — no custom copy layer.
- **Normalization / prepare at ruleset compile:** `geo::algorithm::Validation::is_valid()` gate; invalid rule fails ruleset construction (§34). No auto-repair in v1; no ring re-orientation (geo predicates don't require it).
- **Workspace:** Cargo workspace `core/` (crate `spatial-rules-core`), `node/` (Node-API binding), `benchmarks/`. CLI out of scope.

Assets: [research/01-geometry-stack.md](../research/01-geometry-stack.md) · [ADR-0001](../../../docs/adr/0001-geometry-stack-geo.md).

Handoff to Prepared-geometry options: `geo::PreparedGeometry` is `!Send`/`!Sync` in released 0.33.1 (`Send` lands in an unreleased change); that ticket must reconcile this with the concurrency model (§29).
