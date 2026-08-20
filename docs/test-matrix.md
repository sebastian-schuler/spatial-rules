# Test matrix (regression gate)

Maps each feature area to the test file that owns its coverage (ticket 07). A
post-v1 feature is not "done" unless the file that owns its behavior has a test
for it. This is the enforceable complement to each ticket's `Tests` section.

Shared fixtures (unit-square polygon, rule/candidate builders, jittered ring)
live once in `core/tests/common/mod.rs` and are consumed by every core
integration test (architecture-hardening 07) — the file→coverage-owner mapping
below is unchanged; only the duplication moved into that module.

| Area | Test file | What it owns |
| ---- | --------- | ------------ |
| Where operators (`$eq/$ne/$gt/$gte/$lt/$lte/$in/$nin/$exists/$not`, `$and`/`$or`) | `core/tests/query.rs`, `core/src/where_expr.rs` (unit) | parse + eval semantics, missing/type-mismatch = non-match, indexability |
| Spatial predicates (`intersects/contains/within/covers/covered_by/touches/overlaps`) | `core/tests/query.rs`, `core/src/ruleset.rs` (unit) | directional DE-9IM semantics, boundary-touch cases |
| Overlap area/ratio (`includeOverlap`) | `core/tests/query.rs` | geodesic metrics, ratio bounds, mask invariance, alignment |
| Ruleset build/validation | `core/tests/ruleset.rs`, `core/src/validation.rs` (unit) | ids, envelopes, spatial index, property index, invalid geometry |
| Canonical persistence | `core/tests/ruleset.rs`, `core/tests/engine.rs`, `core/src/ruleset.rs` (unit) | round-trip, fresh id, failed load keeps old ruleset |
| Engine replacement/concurrency | `core/tests/engine.rs` | atomic swap, snapshot semantics, cache invalidation |
| Ingestion | `core/src/ingestion.rs` (unit), `core/tests/edge_matrix.rs` | feature→rule/candidate, id extraction, malformed input |
| Error model | `core/src/error.rs` (unit), `core/tests/error_matrix.rs` | every `SR_*` code reachable by a documented input |
| Edge/input matrix | `core/tests/edge_matrix.rs` | empty, missing id, BOM, NaN/Infinity, antimeridian, unsupported types, skipped property types |
| Property invariants | `core/tests/proptest.rs` | DE-9IM identities, `WhereExpr` eval totality, batch alignment on random inputs |
| Public API surface (exported-name seam) | `core/tests/api_surface.rs` | pins every **exported** function directly (so a rename/drop loses coverage loudly); complements the area owners above: `rule_from_feature`/`candidate_from_feature`, numeric feature ids, `build_spatial_index` + concrete index builders + `query_envelope_into` dedup/reuse, `Query` builders, predicate string round-trips, `Engine::new`/`replace`/`query_mask`, `classify_candidate`, `SpatialError` constructors, prepared-geometry handle, `PropertyValue` ordering |
| Node binding | `node/test/smoke.ts` | mask + rich + overlap + canonical + async + `SR_*` surfacing + dynamic input types + empty batch + single-feature input + `SpatialRulesError` class (Node + Bun) |

## Definition of done

- `cargo test --workspace` green (proptest runs a bounded case count by default).
- `cargo clippy --workspace --all-targets` green.
- Node smoke green under the runtime matrix (Node 22/24/26 + Bun) — `.github/workflows/test.yml`.
- turf cross-check green (`bun run bench cross-check`), with DE-9IM/turf
  disagreements recorded as known quirks in `cross_check.mjs` and ADR-0008.

## Not yet covered (follow-ups)

- **Fuzzing** (`cargo-fuzz` targets over `candidates_from_geojson`,
  `rules_from_geojson`, `Query::from_json`/`WhereExpr::parse`) is a separate
  follow-up sub-task; it needs a nightly toolchain and a fuzz corpus.
