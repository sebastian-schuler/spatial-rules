# Rust core: ruleset compilation

Type: task
Status: resolved
Blocked by: 13

## Question

Build immutable `Ruleset` compilation in `spatial-rules-core` (ADR-0001/0002/0003; use the tdd skill):

- Precomputed rule envelopes + a `SpatialIndex` trait whose default is `rstar::RTree::bulk_load` (ADR-0002).
- Property equality + `$in` indexes for every property at compile; range predicates scanned (ADR-0003).
- `Ruleset` exposing `RuleId` → geometry/properties; fully built before publication, immutable after.

Ruleset builds from a FeatureCollection; indexes queryable; unit tests green.

## Answer

Built immutable `Ruleset` compilation in `spatial-rules-core`, committed to `main`.

**`Ruleset`** (`core/src/ruleset.rs`): `build(Vec<Rule>)` / `build_with(rules, SpatialIndexKind)` / `from_geojson(&str)`. Assigns numeric `RuleId` `0..n-1` in input order; validates each rule geometry (strict reject — propagates `SR_INVALID_GEOMETRY`/`SR_UNSUPPORTED_GEOMETRY_TYPE` with the rule id in the message; duplicate string id → `SR_RULESET_CONSTRUCTION_FAILED`); precomputes `Rect` envelopes; builds indexes. Exposes `len`, `rule_id` (string→numeric), `string_id`, `geometry`, `properties`, `envelope`, `query_envelope`, `property_index`. Fully built before return; no mutation API. Manual `Debug` (internals hold the R-tree, which isn't `Debug`).

**`SpatialIndex`** (`core/src/spatial_index.rs`): trait `query_envelope(&Rect) -> Vec<RuleId>` (sorted, deduped). `RStarIndex` = `rstar 0.13` `RTree::bulk_load` of `GeomWithData<RuleEnvelope, RuleId>` (default); `LinearScanIndex` = envelope scan (ladder baseline); `SpatialIndexKind` + `build_spatial_index` factory so the benchmark ladder can swap scan vs tree.

**`PropertyIndex`** (`core/src/property_index.rs`): compile-time equality index `name → (value → rule ids)`; `matching` (equality) and `matching_in` (`$in`, union sorted+deduped). Range predicates are not indexed (scanned by the query engine, ticket 15).

**`PropertyValue`** gained manual `Eq`/`Ord`/`Hash`/`PartialOrd` (Float via `to_bits` — sound because integral JSON numbers become `Int` and serde_json rejects non-finite values).

**Tests**: 14 new integration tests (`core/tests/ruleset.rs`) + 24 existing = 38 green (`cargo test --workspace`), clippy clean. Seams tested: build/from_geojson, id mapping both directions, geometry/properties access, precomputed envelopes, invalid-geometry and unsupported-type rejection, duplicate-id rejection, rstar envelope queries, rstar-vs-linear-scan agreement, equality/`$in`/absent lookups, empty ruleset, malformed input.

Run: `cargo test --workspace` / `cargo clippy --workspace --all-targets`.

