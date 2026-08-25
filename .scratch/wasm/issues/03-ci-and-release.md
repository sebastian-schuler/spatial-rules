# Build/CI + release automation for the wasm and Python packages

Type: task
Status: resolved

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

## Comments

> *Resolved 2026-08-24: `wasm` CI job (wasm32 target via
> `dtolnay/rust-toolchain`, `wasm-pack build --target bundler`, typecheck, and
> the smoke under node **and** deno) and `python` CI job (maturin release
> wheel + pytest on CPython 3.11 and 3.13); headless-browser deliberately
> deferred to the bundler-target module contract. release-please config +
> manifest extended to `spatial-rules-wasm` (npm, `wasm/` changelog) and
> `spatial-rules` (PyPI, `python/` changelog). The existing rust job gains
> `PYO3_NO_PYTHON=1` so the workspace build needs no interpreter; node/bun
> jobs untouched. 2026-08-25 follow-up: `prebuild-publish.yml` now also
> publishes `spatial-rules-wasm` to npm (wasm-pack build + tsc emit via the
> `prepack` hook) and `spatial-rules` to PyPI (maturin `publish --release
> --skip-existing`, `PYPI_TOKEN` secret) on `v*` tags — the release-automation
> criterion is fully met. 2026-08-25 PR-3 CI fixes (`3d3f907`): the node/bun
> smoke loaded the **published 0.1.1 platform package** (npm install / bun
> auto-install) instead of the freshly built `spatial_rules.node`, so the
> smoke now prefers the local build (`loadNative` order flipped); the rust job
> excludes `spatial-rules-python` (`cargo test/clippy --workspace
> --exclude spatial-rules-python`) because its lib-test links libpython
> (absent under `PYO3_NO_PYTHON`); the python crate enables pyo3
> `extension-module` so the maturin wheel does not link libpython.
> 2026-08-25 release-please fixes (`8f6da80`): PR #4 initially proposed node
> **1.0.0** (breaking changes since 0.1.1) and wasm/python **0.2.0** — both
> wrong per the repo's pre-1.0 policy and the packages' never-published state.
> Config now sets `bump-minor-pre-major: true` for all three (node 0.1.1 →
> 0.2.0), removes the pre-seeded `wasm`/`python` manifest entries so their
> **first release is 0.1.0** (bootstrap), fixes the doubled changelog paths
> (`wasm/wasm/` → `wasm/`), and points the python version at its true source
> (`Cargo.toml` `workspace.package.version` via a toml extra-file, since
> `pyproject.toml` declares `dynamic = ["version"]`). Python gets a distinct
> `spatial-rules-python` component so its tags don't collide with the
> historical node `spatial-rules-v0.1.0` tag.*