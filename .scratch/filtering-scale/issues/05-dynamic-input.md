# Dynamic input types: buffer / string / object

Type: task
Status: resolved

## Answer

Implemented (filtering-scale, 2026-08-20): wrapper-only input normalization
in `node/index.js` — two helpers, `toGeoJsonBuffer` (candidates + rules) and
`toQueryString` (query), applied at the three spec'd entry points only:
`new SpatialRuleset`, `query()`, and `replace()`. A `Buffer` fast-path passes
through untouched (byte-faithful); a string is `Buffer.from`-ed (rules/candidates)
or passed through (query); an object is `JSON.stringify`-ed (value-faithful —
properties preserved, formatting normalized). Anything else — numbers, `null`,
`undefined`, and arrays (not GeoJSON values) — throws a clear wrapper
`TypeError` before the native crossing. The normalized Buffer/query is what
`query()` hands to `QueryResult`, so `toGeoJson()`/`toRichJson()` work on
object/string inputs without a second crossing. Streams are out of scope, and
`node/src/lib.rs` is unchanged (Buffer-in/mask-out stays the boundary; the
benchmark hot path is untouched). `queryRich()`/`queryAsync()`/`fromCanonical()`
are unchanged per scope. Tests: `node/test/smoke.mjs` covers each accepted
input type producing the Buffer form's mask, the query accepting an object,
constructor/replace accepting object + string rules, and unsupported types
(including arrays) throwing a `TypeError`; README documents the accepted input
types and the byte- vs value-faithful nuance. Green under both
`node node/test/smoke.mjs` and `bun node/test/smoke.mjs`.

## Question

The input-side mirror of the chainable output (ticket 03): normalize inputs in
the JS wrapper so callers can pass GeoJSON as a `Buffer`, a GeoJSON **string**,
or a GeoJSON **object**, and the query as a **string** or an **object** —
without changing the byte-oriented native boundary (ADR-0006).

Scope (agreed 2026-08-20; streams excluded):
- `node/index.js` normalizes before the native crossing, with a fast-path
  passthrough when the value is already a `Buffer`:
  - candidates (for `query`) and rules (for `new SpatialRuleset`/`replace`):
    `Buffer` | `string` (GeoJSON text) | `object` (GeoJSON value).
  - `query`: `string` | `object` (stringify when object).
  - Anything else → a clear `TypeError` from the wrapper, not a native error.
- **Streams are out of scope.** The engine is whole-buffer/batch by design, so
  a stream input would only mean collect-then-query: it forces an async path
  and buffers everything — no streaming benefit, and it conflicts with the
  memory contract (ticket 03). If a real consumer ever needs stream input, add
  an async collect-then-query path then, not now.
- Native (`node/src/lib.rs`) is unchanged: Buffer-in/mask-out stays the
  boundary; benchmarks use the raw binding, so the hot path is untouched.
- Doc nuance: `toGeoJson()` on a result built from an object/string input is
  *value*-faithful (properties preserved, formatting normalized), not
  byte-faithful — only a `Buffer` input is byte-faithful.

Tests (`node/test/smoke.mjs`): each accepted input type produces the same mask
as the Buffer form; the query accepts an object; unsupported input types throw
a `TypeError`; constructor/replace accept object + string rules.

Run: `node node/test/smoke.mjs` and `bun node/test/smoke.mjs` — green under
both.
