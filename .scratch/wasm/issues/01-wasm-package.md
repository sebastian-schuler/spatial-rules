# Wasm package: wasm-bindgen core binding + TS glue + npm `spatial-rules-wasm`

Type: task
Status: ready-for-agent

## Question

Ship the engine's Ruleset surface to browser/edge/Deno/Node as a wasm npm
package (`wasm/`, npm `spatial-rules-wasm`), per the decided spec
(`.scratch/wasm/spec.md`). The core is wasm32-ready; nothing in it or the napi
addon changes.

## Agent Brief

**Category:** enhancement
**Summary:** A `wasm/` crate exposing the core's Ruleset-level API via
`wasm-bindgen`, TS glue compiled to `dist/` with a `.d.ts`, packaged as npm
`spatial-rules-wasm` and smoke-tested.

**Current behavior:** The core ships only as the Node napi addon (`node/`).

**Desired behavior:** `wasm-pack build --release --target bundler` produces a
`spatial-rules-wasm` package whose surface is the wrapper's Ruleset subset —
mask as `Uint8Array`, rich JSON as strings — and a smoke proves it.

**Key interfaces:** `spatial-rules-core` (public surface in `core/src/lib.rs`),
the `node/` wrapper's shapes (`GeoJsonInput`/`QueryInput`, mask/rich result
contracts), `node/package.json` as the packaging precedent.

**Acceptance criteria:**
- [ ] `wasm/` Cargo package (`spatial-rules-wasm`, crate-type `cdylib` for
      wasm-bindgen) with `#[wasm_bindgen]` exports: `build(rules)` →
      `SpatialRuleset`; `query`/`resolve` (mask `Uint8Array`); `queryRich`/
      `resolveRich` (JSON strings); `toCanonical`. **No** `replace`/`stats`
      (documented as the read-only subset).
- [ ] TS glue in `wasm/` compiled to `dist/` shipping a `.d.ts`, mirroring the
      wrapper's `GeoJsonInput`/`QueryInput` normalization (reimplemented
      in-package, no `node/` import).
- [ ] npm `spatial-rules-wasm` package (package.json, wasm-pack output,
      `main`/`types` pointing at `dist/`).
- [ ] Wasm smoke (run under node, and deno if available) asserts the
      controlled-ruleset literals (withinDistance `[1,0]`, temporal Monday
      `[1,0,2]`/Tuesday `[0,0,2]`, aggregate count 2 + coverage, resolve
      winner/values/applicable) plus the production `~1k×30` matched count
      (481); no native-addon dependency.
- [ ] Release-build wasm blob size recorded and ≤ ~2 MB raw.

**Out of scope:**
- `replace`/`stats` on wasm (degenerate clock), async surfaces.
- Any change to `spatial-rules-core` or `node/`.
- Python (ticket 02), CI/release wiring (ticket 03).