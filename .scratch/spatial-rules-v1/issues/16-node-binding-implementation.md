# Node binding implementation

Type: task
Status: resolved
Blocked by: 15

## Question

Build the `node/` binding crate with napi-rs per ADR-0006 (execution on the map; use the tdd skill):

- `spatial-rules-core` as a path dependency; `crate-type = ["cdylib"]`; `napi8` feature.
- Hot path: `query(buffer: Buffer, query) -> Uint8Array mask` (byte-in, mask-out); rich object API for per-candidate outcomes (ADR-0004).
- Ruleset construction from a Buffer / FeatureCollection; strict errors → `SpatialRulesError` with `SR_*` codes (ADR-0005).
- Bun smoke test (non-blocking, best-effort per ADR-0006).

Addon loads and its JS tests pass under Node and Bun.

## Answer

Built the `node/` napi-rs binding (napi 3.12 / `napi8`, ADR-0006), committed to `main`.

- **Crate** (`node/`): `spatial-rules-core` path dep; `crate-type = ["cdylib"]`; `napi` 3 (`napi8`) + `napi-derive` 3 + `napi-build`. Exports `SpatialRuleset`.
- **Hot path**: `query(candidates: Buffer, query: JSON string) -> Uint8Array` mask (`0` no match / `1` matched / `2` invalid), aligned to input (ADR-0004).
- **Rich API**: `queryRich(...) -> JSON string` — per-candidate `matched`/`notMatched`/`invalid`, original string rule ids, invalid reason.
- **Construction**: `new SpatialRuleset(Buffer)` from a GeoJSON FeatureCollection.
- **Errors**: native errors carry the `SR_*` code via napi's generic `Error<S>` status; the thin JS wrapper (`node/index.js`) defines `SpatialRulesError extends Error { code }` and re-throws. Verified `.code` = `SR_INVALID_GEOJSON`/`SR_INVALID_QUERY`/`SR_UNSUPPORTED_SPATIAL_PREDICATE`.
- **Tests**: `node/test/smoke.mjs` — construction, mask, `where` filter, rich outcomes, three error codes. Green under Node 24 (`node test/smoke.mjs`) and Bun 1.3.14 (`bun test/smoke.mjs`). Clippy clean.

Build: `cargo build -p spatial-rules-node`, then copy `target/debug/spatial_rules_node.dll` → `node/spatial_rules.node` (see the smoke-test header).

Deferred to later tickets: `replace()` (ticket 19), `queryAsync()` (ADR-0009 now triggers it — follow-up), prebuilt per-platform packages (ticket 18).

