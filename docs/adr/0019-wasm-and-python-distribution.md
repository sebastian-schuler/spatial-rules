# Wasm + Python distribution of the core

The pure-Rust `spatial-rules-core` is wasm32-ready (no I/O, no threading; the
only clock usage is `Engine::replace` observability, off the query path). This
ADR distributes the engine beyond the Node napi addon to two new runtimes —
**wasm** (npm `spatial-rules-wasm`, for Deno/browser/edge/Node ESM) and
**Python** (PyPI `spatial-rules`, PyO3 native) — without changing the core or
the Node addon. The decision model, query shape, and result contracts are
identical everywhere; only the packaging and the set of Engine methods differ.

## Wasm — npm `spatial-rules-wasm`

- `wasm-bindgen` via `wasm-pack`, `--target bundler`: one ESM module consumable
  by browser bundlers, Node ESM, and Deno. The engine is sync and
  whole-buffer, so there is no async story.
- **Ruleset-level surface only** — `build(rules)`, then `query`/`resolve`
  (mask as `Uint8Array`), the rich JSON views (`queryRich`/`resolveRich` as
  JSON strings), and `toCanonical`. **No `replace`/`stats`** (their
  `SystemTime`/`Instant` observability is degenerate on wasm — there is no
  clock) and no async. Documented as the read-only subset of the wrapper.
- Input normalization (`GeoJsonInput` = string | `Uint8Array` | object,
  `QueryInput` = string | object) is reimplemented in-package, decoupled from
  `node/`; the TS glue compiles to `dist/` shipping a `.d.ts` mirroring the
  wrapper's types.
- Release-build wasm blob measured at **829 KB** against a ≤ ~2 MB budget.

## Python — PyPI `spatial-rules`

- PyO3 + maturin, **abi3 `cp39-abi3`** — one wheel for CPython 3.9–3.13.
- **Full Engine surface** with Pythonic types: `Ruleset.from_geojson(rules:
  bytes | str | dict)` → `query`/`resolve` (mask `list[int]`),
  `query_rich`/`resolve_rich` (`list[dict]`), `replace`, `to_canonical`,
  `stats`. Python runs natively, so the clock-backed `replace`/`stats`
  observability is real, not degenerate.
- Internally serializes to exactly the JSON the napi/wasm paths use, so
  semantics are identical across Node/wasm/Python. The rich-JSON serializers
  live in a shared `spatial-rules-bindings-common` crate used by the wasm and
  Python bindings (the node addon's copy stays inline, out of scope).

## Build/CI and release

- `wasm` CI job: rustup `wasm32-unknown-unknown`, `wasm-pack build --release
  --target bundler`, smoke under **node and deno**.
- `python` CI job: maturin release build (abi3), install wheel + pytest on
  CPython **3.11 and 3.13** (catches abi3 drift).
- Headless-browser smoke deferred: the bundler-target module contract (covered
  by Node ESM and Deno) carries the risky part; the engine touches no DOM.
- release-please extended to tag/release `spatial-rules-wasm` (npm) and
  `spatial-rules` (PyPI) from the same Conventional-Commits feed.

## Considered Options

- **Wasm: napi-compatible surface / raw `wasm32-unknown-unknown` exports** —
  rejected: `wasm-bindgen` gives the typed, ergonomic JS surface with one
  bundler-target build; raw exports would reimplement the TS glue and input
  normalization by hand.
- **Wasm: full Engine surface including `replace`/`stats`** — rejected: the
  clock-backed observability is degenerate on wasm (no clock); the read-only
  Ruleset subset is the honest surface and is documented as such.
- **Wasm: async/streaming surfaces** — rejected: the engine is sync and
  whole-buffer; there is nothing to off-thread.
- **Python: separate hand-rolled serializers** — rejected: the bindings share
  the `Query` parse and rich-JSON serializers via
  `spatial-rules-bindings-common` so behavior stays byte-identical.
- **Python: wasm surface parity (no `replace`/`stats`)** — rejected: Python is
  native, so the clock is real; omitting the Engine observability would
  gratuitously diverge from the Node addon.

## Out of scope

- Async/streaming wasm surfaces.
- Any change to `spatial-rules-core` or the Node addon.
- A headless-browser CI job.

## Note — the node addon's inline serializers

The original scoping kept node's rich-JSON serializers inline ("out of scope").
**ADR-0020 supersedes that clause**: the rich-outcome wire contract now lives in
`spatial-rules-bindings-common` and node consumes it, so all three bindings
share one serializer rather than node carrying its own copy.