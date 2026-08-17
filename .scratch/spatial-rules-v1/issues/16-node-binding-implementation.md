# Node binding implementation

Type: task
Status: open
Blocked by: 15

## Question

Build the `node/` binding crate with napi-rs per ADR-0006 (execution on the map; use the tdd skill):

- `spatial-rules-core` as a path dependency; `crate-type = ["cdylib"]`; `napi8` feature.
- Hot path: `query(buffer: Buffer, query) -> Uint8Array mask` (byte-in, mask-out); rich object API for per-candidate outcomes (ADR-0004).
- Ruleset construction from a Buffer / FeatureCollection; strict errors → `SpatialRulesError` with `SR_*` codes (ADR-0005).
- Bun smoke test (non-blocking, best-effort per ADR-0006).

Addon loads and its JS tests pass under Node and Bun.
