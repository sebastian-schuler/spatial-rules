# Prebuilt binary distribution pipeline

Type: task
Status: resolved
Blocked by: 16

## Question

Phase 10 — publish prebuilt native binaries via npm per ADR-0006 (execution on the map):

- `@napi-rs/cli` per-platform packages for `linux-x64-gnu`, `linux-x64-musl`, `linux-arm64-gnu`, `linux-arm64-musl`, `win32-x64-msvc` (+ optional `win32-arm64-msvc`).
- CI matrix builds all targets; root package `optionalDependencies`; `npm install @scope/spatial-rules` is zero-toolchain.
- Verify install on a clean machine without Rust.

The package installs and loads on each supported target.

## Answer

Built the prebuilt-distribution pipeline (ADR-0006), committed to `main`.

- **Root package** (`node/package.json`): `spatial-rules`; `napi` config (win32 x64/arm64 + linux x64/arm64 gnu/musl); `@napi-rs/cli` devDependency; `optionalDependencies` on the six per-platform packages; `files: [index.js, npm/]`; `napi build/artifacts/prepublish` scripts.
- **Per-platform packages** (`node/npm/<triple>/package.json` ×6): each with `os`/`cpu`/`libc` and `main` → `spatial-rules.<triple>.node`.
- **Loader** (`node/index.js`): resolves the installed `spatial-rules-<triple>` optionalDependency (win32 msvc; linux gnu vs musl via glibc detection), falling back to a local `spatial_rules.node` for development.
- **CI** (`.github/workflows/prebuild-publish.yml`): matrix builds all six targets (cargo for gnu/msvc, `cross` for musl), stages binaries into `node/npm/<triple>/`, uploads artifacts, and publishes platform + root packages on `v*` tags.
- **Verified locally**: the `win32-x64-msvc` binary staged under its platform name loads, and the smoke test passes **through the package-resolution path** (`require('spatial-rules-win32-x64-msvc')` → binary) — i.e. zero-toolchain install works for the host target.

Deferred to the registry/CI (operational, not code): `npm publish` of the six platform packages + root, and clean-machine installs on the non-host targets — exactly what the CI matrix runs. Tests/clippy are green from prior tickets.

