# Test matrix (regression gate)

Maps each feature area to the test file that owns its coverage (ticket 07). A
post-v1 feature is not "done" unless the file that owns its behavior has a test
for it. This is the enforceable complement to each ticket's `Tests` section.

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
| Node binding | `node/test/smoke.mjs` | mask + rich + overlap + canonical + async + `SR_*` surfacing (Node + Bun) |

## Definition of done

- `cargo test --workspace` green (proptest runs a bounded case count by default).
- `cargo clippy --workspace --all-targets` green.
- Node smoke green under the runtime matrix (Node 22/24/26 + Bun) — `.github/workflows/test.yml`.
- turf cross-check green (`cd benchmarks/js && node cross_check.mjs`), with DE-9IM/turf
  disagreements recorded as known quirks in `cross_check.mjs` and ADR-0008.

## Not yet covered (follow-ups)

- **Fuzzing** (`cargo-fuzz` targets over `candidates_from_geojson`,
  `rules_from_geojson`, `Query::from_json`/`WhereExpr::parse`) is a separate
  follow-up sub-task; it needs a nightly toolchain and a fuzz corpus.
