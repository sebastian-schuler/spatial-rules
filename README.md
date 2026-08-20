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
  (`intersects` / `contains` / `within` / `covers` / `covered_by` / `touches` /
  `overlaps`), an optional property `where` clause, and optional excluded rule
  ids.
- **Spatial index** — a packed `rstar` R*-tree over rule envelopes, plus a
  linear-scan baseline for the benchmark ladder.
- **Property predicates** — Mongo-style `where`: equality, `$ne`,
  `$gt/$gte/$lt/$lte`, `$in`/`$nin`, `$exists`, `$not`, `$and`/`$or`/`$nor`,
  served by a compile-time equality index with a per-rule fallback.
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

// Rules: a GeoJSON FeatureCollection of polygon rules. Each feature has a
// unique `id`, typed `properties` (queried by `where`), and a Polygon or
// MultiPolygon `geometry`.
const rules = {
  type: 'FeatureCollection',
  features: [
    {
      type: 'Feature',
      id: 'zone-a',
      properties: { active: true, country: 'HR' },
      geometry: { type: 'Polygon', coordinates: [[[0, 0], [0, 10], [10, 10], [10, 0], [0, 0]]] },
    },
  ],
};

const ruleset = new SpatialRuleset(rules); // Buffer | string | object

// Candidates: a GeoJSON FeatureCollection of Polygon / MultiPolygon / Point /
// MultiPoint features.
const candidates = {
  type: 'FeatureCollection',
  features: [
    { type: 'Feature', id: 'c1', properties: { name: 'inside' }, geometry: { type: 'Point', coordinates: [5, 5] } },
  ],
};

// Query: the JSON query object (or its string form) — see "Query shape".
const result = ruleset.query(candidates, {
  spatial: { predicate: 'intersects' },
  where: { active: true, country: { $in: ['HR', 'SI'] } },
  excludeRuleIds: ['zone-b'],
});

// `query()` returns a chainable QueryResult: one evaluation, many output
// views (ADR-0014). See "Outputs" for each terminal's exact type and meaning.
result.toMask();          // Uint8Array
result.toIndices();       // Uint32Array
result.invalidIndices();  // Uint32Array
result.count();           // number
result.summary();         // { matched, notMatched, invalid }
result.toGeoJson();       // string (FeatureCollection)
result.toRichJson();      // string (per-candidate outcomes, lazy)

// Atomic ruleset replacement + observability (ADR-0007): pass another
// FeatureCollection of the same shape to swap the active ruleset.
const report = JSON.parse(ruleset.replace(rules)); // { version, ruleCount, ... }
console.log(ruleset.stats()); // same report shape for the current ruleset
```

### Inputs

| Method | Argument | Accepted JS types | Normalized to |
|---|---|---|---|
| `new SpatialRuleset(rules)` | rules | `Buffer` · `string` · `object` (GeoJSON) | `Buffer` |
| `ruleset.replace(rules)` | rules | `Buffer` · `string` · `object` (GeoJSON) | `Buffer` |
| `ruleset.query(candidates, query)` | candidates | `Buffer` · `string` · `object` (GeoJSON) | `Buffer` |
| `ruleset.query(candidates, query)` | query | `string` · `object` | `string` |
| `ruleset.queryRich(candidates, query)` | candidates / query | `Buffer` / `string` (raw — no normalization) | — |
| `ruleset.queryAsync(candidates, query)` | candidates / query | `Buffer` / `string` (raw — no normalization) | — |
| `ruleset.fromCanonical(rules)` | rules | `Buffer` (canonical JSON from `toJSON()`) | — |

Any other type throws a `TypeError` from the wrapper. A `Buffer` passes
through untouched (byte-faithful); a `string`/`object` is serialized by the
wrapper (value-faithful — properties preserved, formatting normalized).

Rules and candidates are both GeoJSON `FeatureCollection`s:

- **Rules** — Polygon/MultiPolygon only, with a unique `id` and typed
  `properties`; geometries are OGC-validated once at build time.
- **Candidates** — Polygon, MultiPolygon, Point, or MultiPoint; an invalid
  candidate never fails the batch, it is reported per candidate (mask `2`).

### Query shape

The query is a JSON object (or its string form):

```jsonc
{
  "spatial": { "predicate": "intersects" }, // required
  "where": { "active": true },                // optional property filter
  "excludeRuleIds": ["zone-b"],               // optional rule ids to ignore
  "includeOverlap": true                       // optional, rich path only
}
```

- `spatial.predicate` — one of `intersects`, `contains`, `within`, `covers`,
  `covered_by`, `touches`, `overlaps` (DE-9IM; ADR-0008, ADR-0012). Required.
- `where` — a Mongo-style filter over rule `properties` (see below). Optional.
- `excludeRuleIds` — rule ids excluded from the evaluation. Optional.
- `includeOverlap` — when `true`, matched candidates in the rich path also
  carry per-rule geodesic `overlapArea` (m²) / `overlapRatio` ([0, 1]). The
  mask ignores this flag. Optional.

`where` operators:

| Form | Meaning |
|---|---|
| `{ field: value }` or `{ field: { $eq: value } }` | equality (implicit top-level AND over keys) |
| `$ne`, `$gt`, `$gte`, `$lt`, `$lte` | not-equal / ordering |
| `$in`, `$nin` | membership / negated membership |
| `$exists` | key presence |
| `$not: { field: { $op: value } }` | negates one field predicate |
| `$and`, `$or`, `$nor` | boolean composition (`$nor` = whole-clause negation) |

A missing property or a type mismatch is a **non-match** (even for `$ne`); only
malformed predicates throw.

### Outputs

`query()` returns a `QueryResult` (ADR-0014). Every view is aligned to the
input candidate order.

| Method | Returns | Meaning |
|---|---|---|
| `toMask()` | `Uint8Array` | one byte per candidate: `0` no match, `1` matched, `2` invalid |
| `toIndices()` | `Uint32Array` | positions where the mask is `1` (matched) |
| `invalidIndices()` | `Uint32Array` | positions where the mask is `2` (invalid) |
| `count()` | `number` | number of matched candidates |
| `summary()` | `{ matched, notMatched, invalid }` | count breakdown |
| `toGeoJson()` | `string` | matched candidates as a FeatureCollection; original properties preserved (unmatched and invalid are dropped) |
| `toRichJson()` | `string` | per-candidate outcomes as a JSON array (lazy — one native call on first use) |

`toRichJson()` element shapes:

```jsonc
{ "outcome": "matched", "ruleIds": ["zone-a"],
  "overlaps": [{ "ruleId": "zone-a", "overlapArea": 25.0, "overlapRatio": 0.5 }] }
{ "outcome": "notMatched" }
{ "outcome": "invalid", "reason": "..." }
```

`overlaps` appears only when the query set `includeOverlap: true`.

Other methods:

| Method | Returns | Meaning |
|---|---|---|
| `queryRich(candidates, query)` | `string` | the same rich JSON directly (Buffer/string inputs) |
| `queryAsync(candidates, query)` | `Promise<Uint8Array>` | the mask, computed off the main thread |
| `replace(rules)` | `string` | JSON report `{ version, ruleCount, buildDurationMs, lastSwapTime }` |
| `stats()` | `string` | the same report for the current ruleset |
| `toJSON()` | `string` | the ruleset in canonical JSON form (array of rules) |
| `fromCanonical(rules)` | `string` | replace from canonical JSON; returns a report (a failed load keeps the old ruleset) |

### Error codes

Construction and query errors throw a `SpatialRulesError` with a stable `.code`:

| Code | Meaning |
|---|---|
| `SR_INVALID_GEOJSON` | malformed GeoJSON or non-UTF-8 input |
| `SR_INVALID_GEOMETRY` | a rule geometry failed OGC validity |
| `SR_INVALID_QUERY` | structurally invalid query JSON |
| `SR_INVALID_PROPERTY_PREDICATE` | malformed `where` predicate |
| `SR_RULESET_CONSTRUCTION_FAILED` | duplicate rule id, missing bbox, etc. |
| `SR_UNSUPPORTED_GEOMETRY_TYPE` | geometry outside the supported set (rules: Polygon/MultiPolygon; candidates: + Point/MultiPoint) |
| `SR_UNSUPPORTED_SPATIAL_PREDICATE` | predicate outside the supported set |
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
cd node && npm install && npm run typecheck
node --experimental-strip-types test/smoke.ts   # flag needed on Node 22.6+, default-on later
bun  test/smoke.ts

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
