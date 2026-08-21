# spatial-rules

[![npm version](https://img.shields.io/npm/v/spatial-rules)](https://www.npmjs.com/package/spatial-rules)
[![license](https://img.shields.io/github/license/sebastian-schuler/spatial-rules)](LICENSE-MIT)
[![CI](https://img.shields.io/github/actions/workflow/status/sebastian-schuler/spatial-rules/test.yml?branch=main)](https://github.com/sebastian-schuler/spatial-rules/actions/workflows/test.yml)

A high-performance spatial rules/query engine: evaluate batches of candidate
GeoJSON geometries against an indexed, attribute-bearing ruleset, with a Rust
core and a zero-toolchain Node/Bun native addon.

The motivating use case is a batch of candidate geometries checked against a set
of geometry-bearing rules with queryable properties; the library is generic and
knows nothing about any specific application domain.

**One number:** ~18.5 ms to evaluate 1,000 candidates × 30 rules — about **60×
faster** than the equivalent turf.js check (~1.1 s). See
[docs/benchmarks.md](https://github.com/sebastian-schuler/spatial-rules/blob/main/docs/benchmarks.md)
and the summary below.

## Performance vs turf.js

The quick picture: every row compares the engine's full batch query (parse +
spatial predicate + `where` filter + result mask) against a turf.js
implementation of the same check. Both sides assert the same matched count
before timing, and the numbers are the recorded ones — not cherry-picked.

| Workload | turf.js | spatial-rules | speedup |
|---|---|---|---|
| 1,000 candidates × 30 rules (core batch) | 1.11 s | 18.5 ms | **~60×** |
| 300 rules × 1,000 candidates, naive scan | 5.2 s | 5.6 ms | **~940×** |
| 300 rules, strongest JS answer (rbush index + turf) | 15.7 ms | 5.6 ms | ~2.8× |
| 1,000 candidates × 20,000 rules | 61.8 ms | 5.6 ms | 11× |
| Full query over HTTP (`where` + exclusions) | 182 ms | 22.3 ms | ~8× |
| 5,000 candidates, real country boundaries | 14.4 s | 1.9 s | ~7.6× |

The short version:

- **~60× faster** on the reference workload, and the gap **widens as the
  ruleset grows**: the R*-tree bbox index + per-thread prepared geometries
  keep the engine near-flat (4–5.6 ms from 500 to 20,000 rules), while turf
  degrades with every rule (15→62 ms).
- **Even the strongest hand-rolled JS answer loses**: a prebuilt `rbush` index
  + turf relate is still ~2.8× slower at 300 rules — and you'd have to build
  that index yourself.
- **Real-world data**: across a full national boundary file (258 countries,
  546k vertices) per-query cost is independent of rule complexity, because
  geometry is prepared once per ruleset per thread (turf re-does its relate
  work on every call).
- **One honest caveat**: on a tiny query (20 candidates clustered on one
  country), turf's bbox fast-reject dips under the addon's ~5 ms per-call
  floor (parse + FFI). Everywhere realistic, the engine wins — usually by
  10–1,000×.
- **Memory**: the production 30-rule workload peaks at ~65 MB resident
  (comfortably inside a 128 MB limit) and repeated ruleset replacements leak
  nothing. Rulesets size by **rule count**, not coordinate count (~1–3 kB/rule
  — 100k rules ≈ 120–260 MiB of ruleset), and are ~2–5× smaller than a turf.js
  baseline holding the same data; a serving process also holds a per-thread
  prepared-geometry cache on top, which can multiply that several× at high
  rule counts (deferred to the geo 0.34 upgrade).

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
  `2` invalid) for the hot path, and a per-candidate outcomes API with string
  rule ids.
- **Errors** — a stable `SR_*` code model (see below), surfaced in Node as
  `SpatialRulesError`.

## Install

```bash
npm install spatial-rules
```

> Not yet published to npm — the prebuilt-distribution pipeline (per-platform
> optional dependencies + CI matrix + release-please) is in place; pushing a
> `v*` tag triggers the publish. Until then, build the addon from source (see
> [DEVELOPMENT.md](https://github.com/sebastian-schuler/spatial-rules/blob/main/DEVELOPMENT.md)).

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
result.mask();           // Uint8Array
result.indices();        // Uint32Array
result.invalidIndices(); // Uint32Array
result.count();          // number
result.summary();        // { matched, notMatched, invalid }
result.toGeoJson();      // string (FeatureCollection)
result.toOutcomesJson(); // string (per-candidate outcomes, lazy)

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
| `ruleset.queryAsync(candidates, query)` | candidates | `Buffer` · `string` · `object` (GeoJSON) | `Buffer` |
| `ruleset.queryAsync(candidates, query)` | query | `string` · `object` | `string` |
| `ruleset.fromCanonical(rules)` | rules | `Buffer` (canonical JSON from `toCanonical()`) | — |

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
  "includeOverlap": true                       // optional, outcomes path only
}
```

- `spatial.predicate` — one of `intersects`, `contains`, `within`, `covers`,
  `covered_by`, `touches`, `overlaps` (DE-9IM; ADR-0008, ADR-0012). Required.
- `where` — a Mongo-style filter over rule `properties` (see below). Optional.
- `excludeRuleIds` — rule ids excluded from the evaluation. Optional.
- `includeOverlap` — when `true`, matched candidates in the outcomes path also
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
| `mask()` | `Uint8Array` | one byte per candidate: `0` no match, `1` matched, `2` invalid |
| `indices()` | `Uint32Array` | positions where the mask is `1` (matched) |
| `invalidIndices()` | `Uint32Array` | positions where the mask is `2` (invalid) |
| `count()` | `number` | number of matched candidates |
| `summary()` | `{ matched, notMatched, invalid }` | count breakdown |
| `toGeoJson()` | `string` | matched candidates as a FeatureCollection; original properties preserved (unmatched and invalid are dropped) |
| `toOutcomesJson()` | `string` | per-candidate outcomes as a JSON array (lazy — one native call on first use) |

`toOutcomesJson()` element shapes:

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
| `queryAsync(candidates, query)` | `Promise<QueryResult>` | the same chainable result as `query()`, computed off the main thread |
| `replace(rules)` | `string` | JSON report `{ version, ruleCount, buildDurationMs, lastSwapTime }` |
| `stats()` | `string` | the same report for the current ruleset |
| `toCanonical()` | `string` | the ruleset in canonical JSON form (array of rules) |
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

## Requirements

- Node.js **>= 18** (Bun is also supported via the same prebuilt binaries).

## Changelog

See [CHANGELOG.md](CHANGELOG.md).

## Contributing

See [CONTRIBUTING.md](https://github.com/sebastian-schuler/spatial-rules/blob/main/CONTRIBUTING.md)
for how to report issues and submit changes, and
[DEVELOPMENT.md](https://github.com/sebastian-schuler/spatial-rules/blob/main/DEVELOPMENT.md)
for building and testing locally.

## Releasing

See [RELEASING.md](https://github.com/sebastian-schuler/spatial-rules/blob/main/RELEASING.md)
for the release process (release-please →
prebuilt platform binaries → npm publish).

## License

Dual-licensed under [MIT](LICENSE-MIT) and [Apache-2.0](LICENSE-APACHE).
