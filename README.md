# spatial-rules

[![npm version](https://img.shields.io/npm/v/spatial-rules)](https://www.npmjs.com/package/spatial-rules)
[![license](https://img.shields.io/github/license/sebastian-schuler/spatial-rules)](LICENSE-MIT)
[![CI](https://img.shields.io/github/actions/workflow/status/sebastian-schuler/spatial-rules/test.yml?branch=main)](https://github.com/sebastian-schuler/spatial-rules/actions/workflows/test.yml)

A high-performance spatial rules/query engine: evaluate batches of candidate
GeoJSON geometries against an indexed, attribute-bearing ruleset. The Rust core
ships as a Node/Bun native addon, a wasm build for Deno/browser/edge, and a PyO3
wheel for Python, and it is generic. It knows nothing about any application
domain.

New here? [docs/examples.md](https://github.com/sebastian-schuler/spatial-rules/blob/main/docs/examples.md)
walks a city delivery/parking rules engine through matching, `where` filters,
temporal conditions, geofencing, resolution, and aggregation end to end.

**One number:** ~18.5 ms to evaluate 1,000 candidates × 30 rules, about **60×**
faster than the same check in turf.js (~1.1 s). That figure is against a pure-JS
engine; against the native GEOS baseline the engine does not win, see
[Performance vs Python](#performance-vs-python-shapelygeos). Full numbers live in
[docs/benchmarks.md](https://github.com/sebastian-schuler/spatial-rules/blob/main/docs/benchmarks.md).

## Performance vs turf.js

Every row is the engine's full batch query (parse + spatial predicate + `where`
filter + result mask) against a turf.js implementation of the same check, with
both sides asserting the same matched count before timing.

| Workload | turf.js | spatial-rules | speedup |
|---|---|---|---|
| 1,000 candidates × 30 rules (core batch) | 1.11 s | 18.5 ms | **~60×** |
| 300 rules × 1,000 candidates, naive scan | 5.2 s | 5.6 ms | **~940×** |
| 300 rules, strongest JS answer (rbush index + turf) | 15.7 ms | 5.6 ms | ~2.8× |
| 1,000 candidates × 20,000 rules | 61.8 ms | 5.6 ms | 11× |
| Full query over HTTP (`where` + exclusions) | 182 ms | 22.3 ms | ~8× |
| 5,000 candidates, real country boundaries | 14.4 s | 1.9 s | ~7.6× |

The engine prepares each rule's geometry once and indexes the rules with an
R*-tree, so query cost barely moves as rules are added: 4–6 ms from 500 to
20,000 rules, against turf's 15→62 ms. The strongest hand-rolled JS answer, a
prebuilt `rbush` index plus turf relate, is still ~2.8× slower at 300 rules, and
you have to build that index yourself. Across real data (258 countries, 546k
vertices) per-query cost is independent of rule complexity for the same reason.
Turf only comes out ahead on a tiny query: 20 candidates clustered on one
country, where its bbox fast-reject beats the addon's ~5 ms per-call floor
(parse + FFI).

## Performance vs Python (Shapely/GEOS)

Turf's DE-9IM engine (JSTS) is pure JS, which is why it loses. **Shapely 2.x**
wraps **GEOS**, a native C++ engine with prepared geometry and its own spatial
index, so it is a real competitor.

| Workload | Shapely/GEOS | spatial-rules | winner |
|---|---|---|---|
| 1,000 candidates × 30 rules (core batch) | ~3 ms | ~13 ms | **Shapely ~4.5×** |
| 1,000 candidates × 30 rules, naive scan | ~32 ms | ~13 ms | engine ~2.4× |
| 1,000 candidates × 300 rules (indexed) | ~2 ms | ~5 ms | Shapely ~2× |
| 1,000 candidates × 1,000 rules (indexed) | ~2 ms | ~5 ms | Shapely ~2× |

Shapely wins the reference point (~4.5×) and stays ahead as the ruleset grows:
prepared GEOS relate beats the engine's `geo` relate loop on complex
multipolygons, and the engine pays a per-call parse and PyO3 boundary that
Shapely's pre-parsed, pre-indexed setup avoids. Both stay flat with rule count,
each having a real index, so the gap is the relate engine rather than index
scaling. The engine only wins the naive scan (~2.4×). The "thousands of ×" story
belongs to turf.js/JSTS: the PyO3 wheel is a thin binding over the same Rust core
and does not out-run native GEOS on core `intersects`. What it offers over
Shapely is the ruleset model, meaning `where`, DE-9IM predicates, resolution, and
aggregation. The masks are byte-identical across all 1,000 candidates (`bun run
bench python`, release wheel, min-of-3; full picture in
[docs/benchmarks.md §2b](https://github.com/sebastian-schuler/spatial-rules/blob/main/docs/benchmarks.md)).

## Memory

The same synthetic rules held in each stack: the engine's compiled ruleset
against turf's pre-parsed form (feature objects and precomputed bboxes, what the
timed baseline holds). Linux, Bun 1.4.2, 2026-09-17, `bun run bench memory-turf`:

| rules × vertices | engine\* | turf.js |
|---|---|---|
| 1,000 × 10 | 1.8 MiB | 7.9 MiB |
| 1,000 × 100 | 3.2 MiB | 19.3 MiB |
| 1,000 × 1,000 | 16.9 MiB | 80.5 MiB |
| 10,000 × 10 | 8.0 MiB | 24.4 MiB |
| 10,000 × 100 | 21.9 MiB | 83.4 MiB |
| 100,000 × 10 | 71.5 MiB | 122.4 MiB |
| 100,000 × 100 | 208.8 MiB | 640.1 MiB |

\* compiled ruleset — the held footprint. The lazy per-thread prepared-geometry
memo (ADR-0010) adds well under 1 MiB at the default 1,000 candidates, so the
serving footprint is essentially the same.

The ruleset sizes by **rule count**, not coordinate count: ~0.7–2.2 kB per rule
at 100k rules (100k rules ≈ 72–209 MiB), plus ~18 bytes per coordinate. That
makes it ~2–6× smaller than turf's pre-parsed form at normal zoning shapes,
narrowing to ~1.7× on trivial 10-vertex rules where per-rule index overhead
dominates both sides. Serving is workload-proportional, because rule geometry is
prepared lazily on first touch and a process therefore holds only the rules its
queries reach.

The table above measures a **hold**: how much it takes to keep the rules loaded.
A server also does temporary per-request work (buffer the body, parse the
candidates, hand them to the engine, build the response), which is freed after
each request but churns hard. At ~675 requests/second the allocator does not
return memory to the OS fast enough, so the peak climbs well past the hold:
~65 MiB in-process becomes ~138–149 MiB served. Concurrency is not the driver
either; even one request at a time peaks at ~126 MiB.

Of the two numbers, the one that matters is the container's **charged** peak
(`memory.peak`, ~119 MiB), not the process's resident high-water (`VmHWM`,
~138–149 MiB), because the kernel reclaims some of the latter. Size limits off
the charged peak: 119 of 128 MiB is 93%, which fits but leaves ~9 MiB, so use
192–256 MB. For the same work a turf-backed server needs ~199 MiB and is
OOM-killed at 128 MB, at 5× lower throughput. Method and numbers:
[docs/benchmarks.md §HTTP serving memory](https://github.com/sebastian-schuler/spatial-rules/blob/main/docs/benchmarks.md#http-serving-memory-architecture-hardening-09).

## Install

```bash
npm install spatial-rules           # Node/Bun native addon (npm)
npm install spatial-rules-wasm      # wasm: Deno/browser/edge/Node ESM (npm)
pip install spatial-rules           # Python (PyPI)
```

The three packages are the same Rust core, sharing a query shape, result
contracts, and `SR_*` error model, differing only in packaging and method
coverage. The Node/Bun addon is the full engine: `query`/`queryAsync`/`resolve`/
`resolveAsync`, atomic `replace`, `stats`, `toCanonical`/`replaceFromCanonical`,
and the chainable `QueryResult`/`ResolutionResult` views. The wasm build covers
the ruleset-level subset (build, `query`/`resolve`, the rich JSON views, and
`toCanonical`) with no `replace`/`stats`, whose clock-backed observability is
degenerate without a clock, and no async; its release blob is 829 KB. Python
exposes the full engine with Pythonic types.

Requires Node ≥ 18 (Bun works via the same prebuilt binaries), a wasm-ESM
runtime for the wasm build, or CPython 3.9–3.13 (abi3) for the wheel.

### Deno / browser / edge — `spatial-rules-wasm`

```ts
import { SpatialRuleset } from 'spatial-rules-wasm';

const ruleset = new SpatialRuleset(rules); // GeoJSON string | Uint8Array | object
const result = ruleset.query(candidates, query); // mask via result.mask()
console.log(result.toOutcomesJson());
```

### Python — `spatial-rules`

```python
from spatial_rules import Ruleset

ruleset = Ruleset.from_geojson(rules)  # bytes | str | dict
mask = ruleset.query(candidates, query)   # list[int]: 0 no match, 1 matched, 2 invalid
rich = ruleset.query_rich(candidates, query)  # list[dict]
resolved = ruleset.resolve_rich(candidates, query)
print(ruleset.replace(rules))  # dict report
```

## Usage

```js
import { SpatialRuleset } from 'spatial-rules';

// Rules: a GeoJSON FeatureCollection of polygon rules, each with a unique `id`,
// typed `properties` (queried by `where`), and a Polygon/MultiPolygon geometry.
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

// Candidates: a GeoJSON FeatureCollection of Polygon, MultiPolygon, Point, or
// MultiPoint features.
const candidates = {
  type: 'FeatureCollection',
  features: [
    { type: 'Feature', id: 'c1', properties: { name: 'inside' }, geometry: { type: 'Point', coordinates: [5, 5] } },
  ],
};

// Query: the JSON query object (or its string form), see "Query shape".
const result = ruleset.query(candidates, {
  spatial: { predicate: 'intersects' },
  where: { active: true, country: { $in: ['HR', 'SI'] } },
  excludeRuleIds: ['zone-b'],
});

// `query()` returns a chainable QueryResult. One evaluation, many views (see
// "Outputs" for each terminal's exact type and meaning).
result.mask();           // Uint8Array
result.count();          // number
result.toOutcomesJson(); // string (per-candidate outcomes, lazy)
// plus indices(), invalidIndices(), summary(), toGeoJson()

// Atomic ruleset replacement (ADR-0007): pass another FeatureCollection of the
// same shape to swap the active ruleset.
const report = JSON.parse(ruleset.replace(rules)); // { version, ruleCount, ... }
console.log(ruleset.stats()); // same report shape for the current ruleset
```

Rules must be Polygon or MultiPolygon, with a unique `id` and typed
`properties`, and are OGC-validated once at build time. Candidates may also be
Point or MultiPoint, and an invalid candidate never fails the batch: it is
reported per candidate (mask `2`). A `Buffer` input passes through byte-faithful;
a `string` or object is serialized value-faithfully by the wrapper. Any other
type throws a `TypeError`.

### Query shape

The query is a JSON object (or its string form):

```jsonc
{
  "spatial": { "predicate": "intersects" }, // required
  "where": { "active": true },                // optional property filter
  "excludeRuleIds": ["zone-b"],               // optional rule ids to ignore
  "includeOverlap": true,                     // optional, outcomes path only
  "at": "2026-08-24T10:00",                   // optional reference time for $activeAt
  "aggregate": { "count": true, "avg": "speedLimit" } // optional analytics (below)
}
```

- `spatial.predicate` (required): one of `intersects`, `contains`, `within`,
  `covers`, `covered_by`, `touches`, `overlaps` (DE-9IM), or `withinDistance`
  (metric, ADR-0016).
- `spatial.distance`: the `withinDistance` radius in meters; required (finite,
  positive) for `withinDistance`, rejected with any other predicate.
- `where`: a Mongo-style filter over rule `properties` (see below).
- `excludeRuleIds`: rule ids excluded from the evaluation.
- `includeOverlap`: outcomes path only; matched candidates also carry geodesic
  `overlapArea` (m²) / `overlapRatio` ([0, 1]). The mask ignores it.
- `at`: the ISO-8601 reference time (e.g. `2026-08-24T10:00`), required when a
  `$activeAt` predicate is present (ADR-0017).
- `aggregate`: per-candidate analytics over the applicable set (ADR-0018).
  `count`/`coverage` are booleans; `min`/`max`/`sum`/`avg` name a numeric rule
  property, and rules with a missing or non-numeric value are skipped. Rich path
  only.

`where` operators:

| Form | Meaning |
|---|---|
| `{ field: value }` or `{ field: { $eq: value } }` | equality (implicit top-level AND over keys) |
| `$ne`, `$gt`, `$gte`, `$lt`, `$lte` | not-equal / ordering |
| `$in`, `$nin` | membership / negated membership |
| `$exists` | key presence |
| `$not: { field: { $op: value } }` | negates one field predicate |
| `$and`, `$or`, `$nor` | boolean composition (`$nor` = whole-clause negation) |
| `$activeAt: { daysOfWeek, startHour, endHour }` | admits a rule whose window properties (Int bitmask Mon=1..Sun=64; Int hours) contain the query's `at`; requires `at` (ADR-0017) |

A missing property or a type mismatch is a non-match, even for `$ne`; only
malformed predicates throw.

### Outputs

`query()` returns a `QueryResult` (ADR-0014). Every view is aligned to the input
candidate order.

| Method | Returns | Meaning |
|---|---|---|
| `mask()` | `Uint8Array` | one byte per candidate: `0` no match, `1` matched, `2` invalid |
| `indices()` | `Uint32Array` | positions where the mask is `1` (matched) |
| `invalidIndices()` | `Uint32Array` | positions where the mask is `2` (invalid) |
| `count()` | `number` | number of matched candidates |
| `summary()` | `{ matched, notMatched, invalid }` | count breakdown |
| `toGeoJson()` | `string` | matched candidates as a FeatureCollection; original properties preserved (unmatched and invalid are dropped) |
| `toOutcomesJson()` | `string` | per-candidate outcomes as a JSON array (lazy, one native call on first use) |

`toOutcomesJson()` element shapes:

```jsonc
{ "outcome": "matched", "ruleIds": ["zone-a"],
  "overlaps": [{ "ruleId": "zone-a", "overlapArea": 25.0, "overlapRatio": 0.5 }] }
{ "outcome": "notMatched" }
{ "outcome": "invalid", "reason": "..." }
```

`overlaps` appears only when the query set `includeOverlap: true`. When the query
sets `aggregate`, matched candidates also carry an `aggregate` object (absent for
`notMatched`/`invalid`):

```jsonc
{ "outcome": "matched", "ruleIds": ["zone-a"],
  "aggregate": { "count": 1, "min": 30, "max": 30, "sum": 30, "avg": 30, "coverage": 1.0 } }
```

### Resolution (ADR-0015)

`resolve()` and `resolveAsync()` answer "which rule wins, what values apply, and
why" for each candidate. Both return a chainable `ResolutionResult`:

| Method | Returns | Meaning |
|---|---|---|
| `mask()` | `Uint8Array` | one byte per candidate: `0` no resolution, `1` resolved, `2` invalid |
| `count()` | `number` | number of resolved candidates |
| `summary()` | `{ resolved, notResolved, invalid }` | count breakdown |
| `toJson()` | `string` | per-candidate resolution outcomes (lazy, one native call on first use) |

`toJson()` element shapes:

```jsonc
{ "outcome": "resolved", "winner": "zone-a", "values": { "speedLimit": 30 },
  "applicable": [ { "ruleId": "zone-a", "priority": 10,
                    "spatialMatched": true, "propertyMatched": true } ] }
{ "outcome": "notMatched" }
{ "outcome": "invalid", "reason": "..." }
```

The query shape is the same as `query()` (`spatial`/`where`/`excludeRuleIds`,
plus `at`/`distance` as above), and `resolveAsync()` computes off the main
thread. Other methods:

| Method | Returns | Meaning |
|---|---|---|
| `queryAsync(candidates, query)` | `Promise<QueryResult>` | the same chainable result as `query()`, computed off the main thread |
| `resolve(candidates, query)` | `ResolutionResult` | the resolution mask + lazy `toJson()` |
| `resolveAsync(candidates, query)` | `Promise<ResolutionResult>` | resolution computed off the main thread |
| `replace(rules)` | `string` | JSON report `{ version, ruleCount, buildDurationMs, lastSwapTime }` |
| `stats()` | `string` | the same report for the current ruleset |
| `toCanonical()` | `string` | the ruleset in canonical JSON form (array of rules) |
| `replaceFromCanonical(rules)` | `string` | replace from canonical JSON; returns a report (a failed load keeps the old ruleset) |

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

Invalid *candidates* never fail the batch; they produce a `2` in the mask or an
`invalid` outcome with a reason.

## Changelog

See [CHANGELOG.md](CHANGELOG.md).

## Contributing & releasing

[CONTRIBUTING.md](https://github.com/sebastian-schuler/spatial-rules/blob/main/CONTRIBUTING.md)
covers issues and changes, [DEVELOPMENT.md](https://github.com/sebastian-schuler/spatial-rules/blob/main/DEVELOPMENT.md)
covers building and testing locally, and
[RELEASING.md](https://github.com/sebastian-schuler/spatial-rules/blob/main/RELEASING.md)
covers the release process.

## License

Dual-licensed under [MIT](LICENSE-MIT) and [Apache-2.0](LICENSE-APACHE).
