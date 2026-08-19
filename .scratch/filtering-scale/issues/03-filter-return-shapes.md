# Filter return shapes — chainable query result

Type: task
Status: resolved

## Answer

Implemented (filtering-scale, 2026-08-19): the API went **chainable** instead
of one-method-per-return-shape. `query(candidates, query)` now returns a
`QueryResult` (`node/index.js`) whose terminals derive from the single native
evaluation: `toMask()` (Uint8Array), `toIndices()` (Uint32Array, exactly
sized), `invalidIndices()` (Uint32Array of mask==2 positions), `count()`,
`summary()` ({ matched, notMatched, invalid }), `toGeoJson()` (matched
features as a FeatureCollection string, properties preserved from the original
payload — no lossy round-trip), and `toRichJson()` (per-candidate outcomes +
overlap, **lazy** — one native call on first use, ADR-0012; evaluated against
the ruleset current at first call, so a replace() between `query()` and
`toRichJson()` can tear mask vs rich — the mask wins). `query()` is the only
native crossing for the cheap views; the mask stays the primitive.
`replace()`/`queryRich()`/`queryAsync()` are unchanged. Benchmarks use the
raw binding, so the hot-path mask measurement is unaffected. Callers updated:
`node/test/smoke.mjs`, `node/test/clean-install.mjs`, `integration/server.mjs`
(`memory.mjs` discards the query return, so it is unaffected); README
documents the chainable form. Smoke green under Node + Bun; integration
`/query` verified.

Memory contract: `QueryResult` holds the candidates buffer by reference (no
copy); the mask is the minimal primitive (1 byte per candidate). Heavy views
(`toGeoJson`/`toRichJson`) are one-shot — never cached — so results stay lean
for giant lists; prefer `summary()`/index views before materialising them.
Results are short-lived (one request → GC); retaining one retains the
referenced buffer.

## Question

Decide how the filter endpoint gets its output shape without one
method-per-format (the post-v1 "Open proposals": filteredGeojson /
filteredFeatures / object queryRich / keep-indices). Options considered:
separate methods (status quo — proliferation), a `format` option parameter
(return type becomes parameter-dependent), and the chainable result (compute
once, format many; one crossing, lazy rich). Chosen: chainable — `query()`
returns a `QueryResult` with `toMask/toIndices/count/toGeoJson/toRichJson`.
The originally-pending fact (the endpoint's response contract) became moot:
the caller picks the view at the call site.
