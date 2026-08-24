# Python package: PyO3 binding + Pythonic surface + PyPI `spatial-rules`

Type: task
Status: ready-for-agent

## Question

Ship the engine to Python as a native PyO3 package (`python/`, PyPI
`spatial-rules`), per the decided spec (`.scratch/wasm/spec.md`). Python runs
natively, so the full Engine surface — including `replace`/`stats` — is in
scope; the surface is Pythonic (dicts/lists in and out).

## Agent Brief

**Category:** enhancement
**Summary:** A `python/` PyO3 crate exposing the core's full Engine surface,
wrapped in a Pythonic API, packaged by maturin as abi3 wheels and covered by
pytest.

**Current behavior:** No Python distribution of the engine.

**Desired behavior:** `maturin build --release` produces a `cp39-abi3` wheel
for `spatial-rules` whose `Ruleset` class mirrors the wrapper's semantics with
Pythonic types.

**Key interfaces:** `spatial-rules-core` (public surface in `core/src/lib.rs`),
the wrapper's method set (`node/index.ts`) and the napi serializers
(`node/src/lib.rs`) as the semantic reference, `node/package.json`/`bench.mjs`
as packaging/test precedents.

**Acceptance criteria:**
- [ ] `python/` Cargo package (`spatial-rules-python`, PyO3 cdylib, abi3
      `cp39-abi3`) + `pyproject.toml` (maturin) packaging PyPI `spatial-rules`.
- [ ] Pythonic surface: `Ruleset.from_geojson(rules: bytes | str | dict)` →
      `query`/`resolve` (mask `list[int]`), `query_rich`/`resolve_rich`
      (`list[dict]`), `replace`, `to_canonical`, `stats` — dicts in/out,
      JSON serialization identical to the napi path.
- [ ] pytest smoke asserts the controlled-ruleset literals (withinDistance
      `[1,0]`, temporal Monday `[1,0,2]`/Tuesday `[0,0,2]`, aggregate count 2 +
      coverage, resolve winner/values/applicable) plus the production `~1k×30`
      matched count (481), via `maturin develop`.
- [ ] Wheel builds and pytest passes on CPython 3.11 and 3.13 (abi3 drift
      check). No hard wheel-size gate (record actual).

**Out of scope:**
- Async Python surfaces (the engine is sync).
- Any change to `spatial-rules-core` or `node/`.
- Wasm (ticket 01), CI/release wiring (ticket 03).