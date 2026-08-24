# WASM build: distribute the Rust core to browser / edge / Deno / Python

Type: grilling
Status: resolved

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

## Answer

Decided 2026-08-24 (grilling session): two deliverables, both binding the
existing pure-Rust core (no core or Node-addon change).

- **Wasm** (`wasm/`, npm `spatial-rules-wasm`) — `wasm-bindgen` via `wasm-pack`,
  `--target bundler`. **Ruleset-level surface only**: `build`/`query` (mask
  `Uint8Array`)/`resolve`/rich JSON strings/`toCanonical`. No `replace`/`stats`
  (degenerate clock on wasm), no async. Same `GeoJsonInput`/`QueryInput`
  normalization; TS glue → `dist/` + `.d.ts`, normalization reimplemented
  in-package. Soft size budget ≤ ~2 MB raw wasm (record actual).
- **Python** (`python/`, PyPI `spatial-rules`) — PyO3 + maturin, abi3
  `cp39-abi3`, sync-only. Pythonic shape (`Ruleset.from_geojson(rules)` →
  `.query/.resolve/.query_rich/.resolve_rich/.replace/.to_canonical/.stats`,
  dicts/lists in and out). Full Engine surface (native clock, so replace/stats
  work).
- **Build/CI** — `wasm` job (wasm32 target + wasm-pack + smoke under node and
  deno) and `python` job (maturin + pytest on CPython 3.11 + 3.13);
  headless-browser smoke deferred (the bundler-target module contract covers
  it); release-please extended to both packages from the same feed.
- **Smoke content (all runtimes)** — the node smoke's controlled-ruleset
  literals (withinDistance `[1,0]`, temporal Monday `[1,0,2]`/Tuesday `[0,0,2]`,
  aggregate count 2 + coverage, resolve winner/values/applicable) plus the
  production `~1k×30` matched-count literal (481); no native-addon dependency.
- **Out of scope** — async/streaming wasm surfaces; changing the core or the
  Node addon.

Graduated to `.scratch/wasm/` (spec + implementation tickets).

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