# WASM build: distribute the Rust core to browser / edge / Deno / Python

Type: grilling
Status: ready-for-human

## Question

Per `docs/roadmap.md` fog: "**WASM build** — same engine in
Node/Bun/Deno/browser/edge/Python. Distribution concern, not a design one; the
Rust core is ready whenever." The core (`spatial-rules-core`) is pure Rust with
no I/O or threading, so it should compile to `wasm32`. Before building, decide
the distribution shape (the grill frontier):

- **Q1 — The binding**: `wasm-bindgen` (with `wasm-pack`) vs raw
  `wasm32-unknown-unknown` exports vs a napi-compatible surface. The engine is
  sync and whole-buffer, so there is no async story to reconcile on the wasm
  side.
- **Q2 — The surface**: which of the wrapper API ships on wasm — `query`
  (mask), `resolve`, `withinDistance`, temporal, aggregation — and the JS glue
  shape (the same `GeoJsonInput`/`QueryInput` normalization? mask as
  `Uint8Array`?).
- **Q3 — Packaging**: an npm package for Deno/browser/edge; Python (PyO3, a
  separate concern from wasm) — is a Python binding in scope or deferred? A
  size budget for the wasm blob.
- **Q4 — Build/CI**: adding the `wasm32` target to the release pipeline and a
  smoke test per platform.

Once decided, this graduates to `.scratch/wasm/` with implementation tickets.

## Comments

> *Roadmap fog item — "the Rust core is ready whenever".*

## Agent Brief

**Category:** enhancement
**Summary:** Decide and then implement a WASM distribution of the Rust core for browser/edge/Deno (and possibly a Python binding).

**Current behavior:** The core ships only as the Node/Bun napi addon.

**Desired behavior:** A wasm build of the core with a documented JS surface and packaging.

**Key interfaces:** `spatial-rules-core` (pure Rust, no I/O), the wrapper's input/output shapes.

**Acceptance criteria (post-decision):**
- [ ] The binding + packaging decision is recorded
- [ ] A wasm build of the core compiles and passes a smoke test
- [ ] The JS surface (mask/rich JSON) is documented

**Out of scope:**
- Async/streaming wasm surfaces (the engine is sync and whole-buffer)
- Changing the core or the Node addon