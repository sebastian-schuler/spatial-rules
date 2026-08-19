# spatial-rules-engine

A high-performance spatial rules/query engine: evaluate batches of candidate
GeoJSON geometries against an indexed, attribute-bearing ruleset, with a Rust
core and a zero-toolchain Node/Bun native addon.

The motivating use case is a batch of candidate geometries checked against a set
of geometry-bearing rules with queryable properties; the library is generic and
knows nothing about any specific application domain.

**One number:** ~20 ms to evaluate 1,000 candidates × 30 rules — about **52×
faster** than the equivalent turf.js check (~1.1 s). See
[`docs/benchmarks.md`](docs/benchmarks.md).

## What it does

- **Ruleset** — an immutable, query-optimized collection of geometry-bearing
  rules with typed properties, built and validated once, then atomically
  replaceable at runtime without dropping in-flight queries.
- **Query** — one batch evaluation: a spatial predicate
  (`intersects` / `contains` / `within`), an optional property `where` clause,
  and optional excluded rule ids.
- **Spatial index** — a packed `rstar` R*-tree over rule envelopes, plus a
  linear-scan baseline for the benchmark ladder.
- **Property predicates** — Mongo-style `where`: equality, `$ne`,
  `$gt/$gte/$lt/$lte`, `$in`, `$and`/`$or`, served by a compile-time equality
  index with a per-rule fallback.
- **Result model** — a compact `Uint8Array` mask (`0` no match, `1` matched,
  `2` invalid) for the hot path, and a rich per-candidate API with string rule
  ids.
- **Errors** — a stable `SR_*` code model (see below), surfaced in Node as
  `SpatialRulesError`.

## Install

```bash
npm install spatial-rules
```

> Not yet published to npm — the prebuilt-distribution pipeline (per-platform
> optional dependencies + CI matrix) is in place; registry publish is the
> remaining operational step. Until then, build the addon from source (below).

## Usage

```js
import { SpatialRuleset } from 'spatial-rules';

const ruleset = new SpatialRuleset(rulesGeojsonBuffer);

// One query, many output views (chainable result): the mask is the primitive,
// and every other view is derived from the same evaluation.
const result = ruleset.query(candidatesGeojsonBuffer, JSON.stringify({
  spatial: { predicate: 'intersects' },
  where: { active: true, country: { $in: ['HR', 'SI'] } },
  excludeRuleIds: excludedRuleIds,
}));
result.toMask()       // Uint8Array mask (0 / 1 / 2), aligned to the input
result.toIndices()    // Uint32Array of matched candidate indices
result.count()        // number of matched candidates
result.summary()      // { matched, notMatched, invalid } count breakdown
result.invalidIndices() // Uint32Array of invalid candidate indices
result.toGeoJson()    // matched candidates as a GeoJSON FeatureCollection string
result.toRichJson()   // per-candidate outcomes + overlap, as a JSON string (lazy)

// Atomic ruleset replacement + observability (ADR-0007).
const report = JSON.parse(ruleset.replace(newRulesGeojsonBuffer));
console.log(ruleset.stats()); // { version, ruleCount, buildDurationMs, lastSwapTime }
```

Both `candidates` and `rules` are GeoJSON `FeatureCollection`s, accepted as a
`Buffer`, a GeoJSON string, or a GeoJSON object; `query` is the JSON query
shape above, accepted as a string or an object. The wrapper normalizes
candidates and rules to a `Buffer` and the query to a string before the native
crossing: a `Buffer` passes through untouched (byte-faithful), while a string
or object is value-faithful — properties are preserved but formatting is
normalized by the wrapper's serialization and by `toGeoJson()`. Any other type
throws a `TypeError`.

### Error codes

Construction and query errors throw a `SpatialRulesError` with a stable `.code`:

| Code | Meaning |
|---|---|
| `SR_INVALID_GEOJSON` | malformed GeoJSON or non-UTF-8 input |
| `SR_INVALID_GEOMETRY` | a rule geometry failed OGC validity |
| `SR_INVALID_QUERY` | structurally invalid query JSON |
| `SR_INVALID_PROPERTY_PREDICATE` | malformed `where` predicate |
| `SR_RULESET_CONSTRUCTION_FAILED` | duplicate rule id, missing bbox, etc. |
| `SR_UNSUPPORTED_GEOMETRY_TYPE` | geometry outside Polygon/MultiPolygon |
| `SR_UNSUPPORTED_SPATIAL_PREDICATE` | predicate other than intersects/contains/within |
| `SR_UNSUPPORTED_PROPERTY_OPERATOR` | operator outside the Mongo subset |
| `SR_NATIVE` | unexpected native/runtime failure |

Invalid *candidates* never fail the batch — they produce a `2` in the mask /
an `invalid` outcome with a reason.

## Repository layout

```
core/        pure-Rust engine (Ruleset, Engine, query pipeline, indexes)
node/        napi-rs addon + JS wrapper + per-platform npm packages
benchmarks/  criterion algorithm ladder + turf.js baseline + dataset
integration/ Bun + Express app + Docker image + memory harness
docs/        CONTEXT.md, Initial-plan.md, benchmarks.md, adr/
```

## Development

```bash
# Rust core + binding
cargo test --workspace
cargo clippy --workspace --all-targets

# Node/Bun binding smoke (build the addon first)
cargo build -p spatial-rules-node
# Windows: copy target/release/spatial_rules_node.dll -> node/spatial_rules.node
# Linux:   copy target/release/libspatial_rules_node.so -> node/spatial_rules.node
node node/test/smoke.mjs
bun  node/test/smoke.mjs

# Benchmarks + integration — one dispatcher at the repo root
bun install                        # once: harness deps (turf, rbush, express)
bun run bench                      # list every command
bun run bench build                # build binding (+ copy) + cross_check binary
bun run bench cross-check && bun run bench perf
bun run bench all                  # full battery

# Docker integration (server + memory measurement)
docker build -f integration/Dockerfile -t spatial-rules .
docker run --rm --memory=128m -p 3000:3000 spatial-rules
```

### Configuration

The benchmark and integration harnesses read all configuration from the single
committed `benchmarks.json` at the repo root; per-run tweaks are `--flag=value`
arguments (e.g. `bun run bench crossover --sizes=20,200,1000,5000`). There are
**no environment variables and no `.env` files** — every knob is either in
`benchmarks.json` or passed as a flag. See
[`docs/benchmarks.md`](docs/benchmarks.md) for the full key → flag map.

The core engine and the `node/` addon read no configuration at all; their
input travels through the API only.

## Docs

- [`CONTEXT.md`](CONTEXT.md) — domain glossary (single source of vocabulary).
- [`docs/Initial-plan.md`](docs/Initial-plan.md) — implementation spec.
- [`docs/adr/`](docs/adr/) — architecture decision records.
- [`docs/benchmarks.md`](docs/benchmarks.md) — perf and memory evidence.
