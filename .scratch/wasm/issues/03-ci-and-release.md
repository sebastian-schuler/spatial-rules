# Build/CI + release automation for the wasm and Python packages

Type: task
Status: ready-for-agent

## Question

Wire the two new packages into the release pipeline, per the decided spec
(`.scratch/wasm/spec.md`): CI jobs that build and smoke them, and
release-please covering both new packages from the same Conventional-Commits
feed.

## Agent Brief

**Category:** ci
**Summary:** A `wasm` CI job (wasm32 target + wasm-pack + node/deno smoke) and a
`python` job (maturin + pytest on 3.11/3.13), plus release-please wiring for
npm `spatial-rules-wasm` and PyPI `spatial-rules`.

**Current behavior:** CI (`.github/workflows/test.yml`) covers Rust tests/clippy,
the Node addon (build + typecheck + node/bun smoke). No wasm, no Python.

**Desired behavior:** Both new jobs run green in CI; a release triggers
published npm and PyPI packages.

**Key interfaces:** `.github/workflows/test.yml`, `bench.mjs`/`benchmarks.json`
(harness precedents), the release-please config in the repo
(`.release-please-manifest.json`/`release-please-config.json` if present),
`wasm/` and `python/` package outputs (tickets 01/02).

**Acceptance criteria:**
- [ ] `wasm` job: `rustup target add wasm32-unknown-unknown`, `wasm-pack build
      --release --target bundler` (in `wasm/`), run the wasm smoke under node
      **and** deno.
- [ ] `python` job: maturin release build (abi3), `maturin develop` + pytest on
      CPython 3.11 and 3.13.
- [ ] Headless-browser smoke deliberately deferred (recorded in the workflow
      as the bundler-target module contract).
- [ ] release-please config extends to `spatial-rules-wasm` (npm) and
      `spatial-rules` (PyPI) from the same feed; the existing Rust/node jobs
      untouched.

**Out of scope:**
- The packages themselves (tickets 01/02).
- Any change to `spatial-rules-core` or `node/`.
- A headless-browser CI job.