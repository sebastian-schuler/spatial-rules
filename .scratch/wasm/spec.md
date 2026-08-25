# WASM + Python distribution — same engine, more runtimes

Decision record from the grilling session (`.scratch/remaining-work/issues/04`).
The pure-Rust `spatial-rules-core` is wasm32-ready (no I/O, no threading; the
only clock usage is `Engine::replace` observability, off the query path). This
effort distributes the engine to Deno/browser/edge (wasm) and Python (native),
without changing the core or the Node napi addon.

## Two deliverables

### Wasm — npm `spatial-rules-wasm` (in `wasm/`)

- `wasm-bindgen` via `wasm-pack`, `--target bundler` (one ESM consumable by
  browser bundlers, Node ESM, and Deno). The engine is sync and whole-buffer,
  so there is no async story.
- **Ruleset-level surface only** — `build(rules)`, then `query` (mask as
  `Uint8Array`), `resolve`, the rich JSON views (`queryRich`/`resolveRich` as
  JSON strings), and `toCanonical`. **No `replace`/`stats`** (their
  `SystemTime`/`Instant` observability is degenerate on wasm — no clock), no
  async. Documented as the read-only subset of the wrapper.
- Same input normalization as the Node wrapper: `GeoJsonInput` (Buffer | string
  | object) and `QueryInput` (string | object), reimplemented **in-package**
  (decoupled from `node/`). TS glue compiled to `dist/` shipping a `.d.ts`
  mirroring the wrapper's types.
- Soft size budget: **≤ ~2 MB raw wasm** (native cdylib is 2.2 MB); record the
  actual release-build size.

### Python — PyPI `spatial-rules` (in `python/`)

- PyO3 + maturin, **abi3 `cp39-abi3`** wheels (one wheel for CPython 3.9–3.13),
  sync-only.
- Pythonic surface: `Ruleset.from_geojson(rules: bytes | str | dict)` → class
  with `query(candidates, query) -> dict` (mask as `list[int]`),
  `resolve(...)`, `query_rich(...) -> list[dict]`, `resolve_rich(...)`,
  `replace(...) -> dict`, `to_canonical() -> dict`, `stats() -> dict`.
  **Full Engine surface** — Python runs natively, so the clock-backed
  replace/stats observability is real, not degenerate.
- Internally serializes to exactly the JSON the napi path uses, so semantics
  are identical across Node/wasm/Python.

## Build/CI

- **Wasm job** — rustup `wasm32-unknown-unknown`, `wasm-pack build --release
  --target bundler`, run the smoke under **node and deno**.
- **Python job** — maturin release build (abi3), `maturin develop` + pytest on
  CPython **3.11 + 3.13** (catches abi3 drift).
- **Headless-browser smoke deferred** — the bundler-target module contract
  (exercised by Node ESM and Deno) covers the risky part; this engine touches
  no DOM/WebGL.
- **Release automation** — release-please extended to tag/release
  `spatial-rules-wasm` (npm) and `spatial-rules` (PyPI) from the same
  Conventional-Commits feed.

## Smoke content (identical intent on every runtime)

The node smoke's controlled-ruleset literals — withinDistance `[1,0]`, temporal
Monday `[1,0,2]` / Tuesday `[0,0,2]`, aggregate count 2 + coverage, resolve
winner/values/applicable — **plus** the production `~1k×30` matched-count
literal (481). No native-addon dependency, so each package's job stands alone.

## Out of scope

- Async/streaming wasm surfaces (the engine is sync and whole-buffer).
- Changing the core or the Node addon.