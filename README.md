# spatial-rules

[![npm version](https://img.shields.io/npm/v/spatial-rules)](https://www.npmjs.com/package/spatial-rules)
[![license](https://img.shields.io/github/license/sebastian-schuler/spatial-rules)](LICENSE-MIT)
[![CI](https://img.shields.io/github/actions/workflow/status/sebastian-schuler/spatial-rules/test.yml?branch=main)](https://github.com/sebastian-schuler/spatial-rules/actions/workflows/test.yml)

A high-performance spatial rules/query engine: evaluate batches of candidate
GeoJSON geometries against an indexed, attribute-bearing ruleset, with a Rust
core distributed three ways — a zero-toolchain Node/Bun native addon, a wasm
build for Deno/browser/edge, and a PyO3 wheel for Python (ADR-0019).

The motivating use case is a batch of candidate geometries checked against a set
of geometry-bearing rules with queryable properties; the library is generic and
knows nothing about any specific application domain.

> **New here?** Jump to the end-to-end walkthrough — a city delivery/parking
> rules engine exercising matching, `where` filters, temporal conditions,
> geofencing, resolution, and aggregation together:
> [docs/examples.md](https://github.com/sebastian-schuler/spatial-rules/blob/main/docs/examples.md).

**One number:** ~18.5 ms to evaluate 1,000 candidates × 30 rules — about **60×
faster** than the equivalent turf.js check (~1.1 s). See
[docs/benchmarks.md](https://github.com/sebastian-schuler/spatial-rules/blob/main/docs/benchmarks.md)
and the summary below. Note: that 60× is measured against turf.js (a pure-JS
JSTS engine); against a native C++ baseline (Shapely/GEOS) the engine does not
win — see [Performance vs Python](#performance-vs-python-shapelygeos) below.

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
- **Memory**: the production 30-rule workload peaks at ~67 MB resident
  (Linux container, comfortably inside a 128 MB limit) and repeated ruleset
  replacements leak nothing — proven to 50 swaps at 100k rules on Linux
  (bounded sawtooth, not a leak). Rulesets size by **rule count**, not
  coordinate count (~1–3 kB/rule — 100k rules ≈ 120–260 MiB of ruleset), are
  ~2–5× smaller than a turf.js baseline holding the same data, and prepare
  rule geometries **lazily on first touch** — so a serving process's footprint
  is proportional to the rules queries actually touch, not the whole ruleset
  (the 100k×100 serving footprint dropped from ~1.8 GiB to ~282 MiB at 1,000
  candidates). Worst case (touch-everything workloads) and the per-thread
  duplication remain at the pre-lazy ceiling (geo 0.34 deferral).

## Performance vs Python (Shapely/GEOS)

The turf.js comparison is the JS story — but turf runs a **pure-JS** DE-9IM
engine (JSTS), which is *why* it's 60×+ slower. The standard Python competitor,
**Shapely 2.x**, wraps **GEOS** — a mature native C++ DE-9IM engine with its own
prepared geometry and spatial-index machinery — so it is a genuinely strong
opponent. The full picture is in
[docs/benchmarks.md §2b](https://github.com/sebastian-schuler/spatial-rules/blob/main/docs/benchmarks.md);
the honest summary (`bun run bench python`, release PyO3 wheel, min-of-3):

| Workload | Shapely/GEOS | spatial-rules | winner |
|---|---|---|---|
| 1,000 candidates × 30 rules (core batch) | ~3 ms | ~13 ms | **Shapely ~4.5×** |
| 1,000 candidates × 30 rules, naive scan | ~32 ms | ~13 ms | engine ~2.4× |
| 1,000 candidates × 300 rules (indexed) | ~2 ms | ~5 ms | Shapely ~2× |
| 1,000 candidates × 1,000 rules (indexed) | ~2 ms | ~5 ms | Shapely ~2× |

The engine's ~13 ms on the reference matches its own criterion ladder's
relate-only rung; the masks are byte-identical between the engine and Shapely
(verified across all 1,000 candidates), so this is a real result, not a
measurement artifact.

The short version, honestly:

- **Against a pure-JS engine, the engine wins big** (60–1,000× vs turf.js).
- **Against a native C++ GEOS baseline, Shapely wins the reference point**
  (~4.5×) and stays ahead as the ruleset grows. Why: prepared GEOS relate
  beats the engine's `geo` relate loop on complex multipolygons, and the engine
  pays a per-call parse + PyO3 boundary that Shapely's pre-parsed, pre-indexed
  setup avoids. Both sides stay flat with rule count — each has a real index —
  so the gap is the relate engine, not index scaling.
- The engine still wins the **naive scan** (~2.4×) — that rung is the only one
  where the engine beats Shapely — but even its best case is a fraction of the
  margin it had over turf.
- **Bottom line**: the "thousands of ×" claim is specific to turf.js/JSTS. The
  PyO3 wheel is a thin Python binding over the same Rust core, so it does not
  out-run a native GEOS-backed library on core `intersects`. The engine's value
  against Shapely is its **ruleset model** — the Mongo-style `where`, DE-9IM
  predicates, resolution, and aggregation — not raw DE-9IM throughput.

## What it does

- **Ruleset** — an immutable, query-optimized collection of geometry-bearing
  rules with typed properties, built and validated once, then atomically
  replaceable at runtime without dropping in-flight queries.
- **Query** — one batch evaluation: a spatial predicate (`intersects` /
  `contains` / `within` / `covers` / `covered_by` / `touches` / `overlaps`, or
  the metric `withinDistance` with a radius), an optional property `where`
  clause, optional excluded rule ids, and an optional reference time `at` for
  temporal predicates.
- **Spatial index** — a packed `rstar` R*-tree over rule envelopes, plus a
  linear-scan baseline for the benchmark ladder.
- **Property predicates** — Mongo-style `where`: equality, `$ne`,
  `$gt/$gte/$lt/$lte`, `$in`/`$nin`, `$exists`, `$not`, `$and`/`$or`/`$nor`,
  and the temporal `$activeAt` window predicate, served by a compile-time
  equality index with a per-rule fallback.
- **Result model** — a compact `Uint8Array` mask (`0` no match, `1` matched,
  `2` invalid) for the hot path, and a per-candidate outcomes API with string
  rule ids.
- **Resolution** — `resolve()`/`resolveAsync()` answer "which rule wins, what
  values apply, and why" per candidate: an ordered applicable set, its winner,
  and first-provider-wins derived values (ADR-0015).
- **Errors** — a stable `SR_*` code model (see below), surfaced in Node as
  `SpatialRulesError`.

## Install

```bash
npm install spatial-rules           # Node/Bun native addon (npm)
npm install spatial-rules-wasm      # wasm: Deno/browser/edge/Node ESM (npm)
pip install spatial-rules           # Python (PyPI)
```

The engine ships in three flavors (ADR-0019): the zero-toolchain Node/Bun
native addon `spatial-rules`, a wasm build `spatial-rules-wasm` for
Deno/browser/edge, and a PyO3 native wheel `spatial-rules` on PyPI. The wasm
and Python packages publish from the same release pipeline (see
[RELEASING.md](https://github.com/sebastian-schuler/spatial-rules/blob/main/RELEASING.md)).

## Distributions

The same Rust core, three surfaces (ADR-0019). Every surface shares the query
shape, result contracts, and `SR_*` error model; they differ in packaging and
which Engine methods are in scope.

### Node/Bun — `spatial-rules` (native addon)

The full engine: `SpatialRuleset` with `query`/`queryAsync`/`resolve`/
`resolveAsync`, atomic `replace`, `stats`, `toCanonical`/`replaceFromCanonical`, and
the `QueryResult`/`ResolutionResult` chainable views. This README's examples
use this surface.

### Deno / browser / edge — `spatial-rules-wasm` (wasm)

```bash
npm install spatial-rules-wasm
```

```ts
import { SpatialRuleset } from 'spatial-rules-wasm';

const ruleset = new SpatialRuleset(rules); // GeoJSON string | Uint8Array | object
const result = ruleset.query(candidates, query); // mask via result.mask()
console.log(result.toOutcomesJson());
```

The **Ruleset-level subset** of the wrapper: `build` (`new SpatialRuleset`),
`query`/`resolve` (mask as `Uint8Array`), the rich JSON views
(`toOutcomesJson`/`toJson`), and `toCanonical`. Inputs accept
`GeoJSON string | Uint8Array | object` and queries accept `string | object`
(reimplemented in-package). **No `replace`/`stats`** — their clock-backed
observability is degenerate on wasm (no clock) — and **no async** (the engine
is sync and whole-buffer). Errors surface as `SpatialRulesError` with the same
`SR_*` codes. Release wasm blob: 829 KB.

### Python — `spatial-rules` (PyPI)

```bash
pip install spatial-rules
```

```python
from spatial_rules import Ruleset

ruleset = Ruleset.from_geojson(rules)  # bytes | str | dict
mask = ruleset.query(candidates, query)   # list[int]: 0 no match, 1 matched, 2 invalid
rich = ruleset.query_rich(candidates, query)  # list[dict]
resolved = ruleset.resolve_rich(candidates, query)
print(ruleset.replace(rules))  # dict report
```

The **full Engine surface** with Pythonic types: `Ruleset.from_geojson`
(`bytes | str | dict`), `query`/`resolve` (mask as `list[int]`),
`query_rich`/`resolve_rich` (`list[dict]`), `replace`, `to_canonical`, and
`stats` — dicts/lists in and out. Python runs natively, so the clock-backed
`replace`/`stats` observability is real. JSON serialization is identical to
the napi/wasm paths. Errors raise `SpatialRulesError` with the `SR_*` code in
the message.

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
| `ruleset.replaceFromCanonical(rules)` | rules | `Buffer` (canonical JSON from `toCanonical()`) | — |

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
  "includeOverlap": true,                     // optional, outcomes path only
  "at": "2026-08-24T10:00",                   // optional reference time for $activeAt
  "aggregate": { "count": true, "avg": "speedLimit" } // optional analytics (below)
}
```

- `spatial.predicate` — one of `intersects`, `contains`, `within`, `covers`,
  `covered_by`, `touches`, `overlaps` (DE-9IM; ADR-0008, ADR-0012), or
  `withinDistance` (metric; ADR-0016). Required.
- `spatial.distance` — the `withinDistance` radius in meters; required (a
  finite positive number) when the predicate is `withinDistance`, rejected with
  any other predicate. Optional.
- `where` — a Mongo-style filter over rule `properties` (see below). Optional.
- `excludeRuleIds` — rule ids excluded from the evaluation. Optional.
- `includeOverlap` — when `true`, matched candidates in the outcomes path also
  carry per-rule geodesic `overlapArea` (m²) / `overlapRatio` ([0, 1]). The
  mask ignores this flag. Optional.
- `at` — the reference time (ISO-8601, e.g. `2026-08-24T10:00`), required when
  a `$activeAt` predicate is present; a present-but-unused `at` is validated and
  ignored. Optional (ADR-0017).
- `aggregate` — per-candidate analytics over the applicable rule set
  (ADR-0018): `count`/`coverage` are booleans, `min`/`max`/`sum`/`avg` each
  name a rule-property field (`{ count: true, min: "speedLimit", coverage: true }`).
  `min`/`max`/`sum`/`avg` fold the named numeric property across applicable
  rules (missing/non-numeric rules skipped; absent if nothing contributes);
  `coverage` is the geodesic fraction of the candidate covered by the union of
  applicable rules. Computed on the rich path only. Optional.

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

When the query sets `aggregate`, matched candidates also carry an `aggregate`
object (absent for `notMatched`/`invalid`):

```jsonc
{ "outcome": "matched", "ruleIds": ["zone-a"],
  "aggregate": { "count": 1, "min": 30, "max": 30, "sum": 30, "avg": 30, "coverage": 1.0 } }
```

### Resolution (ADR-0015)

`resolve()` / `resolveAsync()` answer "which rule wins, what values apply, and
why" for each candidate. Both return a chainable `ResolutionResult`:

| Method | Returns | Meaning |
|---|---|---|
| `mask()` | `Uint8Array` | one byte per candidate: `0` no resolution, `1` resolved, `2` invalid |
| `count()` | `number` | number of resolved candidates |
| `summary()` | `{ resolved, notResolved, invalid }` | count breakdown |
| `toJson()` | `string` | per-candidate resolution outcomes (lazy — one native call on first use) |

`toJson()` element shapes:

```jsonc
{ "outcome": "resolved", "winner": "zone-a", "values": { "speedLimit": 30 },
  "applicable": [ { "ruleId": "zone-a", "priority": 10,
                    "spatialMatched": true, "propertyMatched": true } ] }
{ "outcome": "notMatched" }
{ "outcome": "invalid", "reason": "..." }
```

The query shape is the same as `query()` (`spatial`/`where`/`excludeRuleIds`,
plus `at`/`distance` as above); `resolveAsync()` computes off the main thread.

Other methods:

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

Invalid *candidates* never fail the batch — they produce a `2` in the mask /
an `invalid` outcome with a reason.

## Requirements

- Node/Bun (`spatial-rules`): Node.js **>= 18** (Bun also supported via the
  same prebuilt binaries).
- Deno/browser/edge (`spatial-rules-wasm`): any runtime with wasm ESM support.
- Python (`spatial-rules`): CPython **3.9–3.13** (abi3 wheel).

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
