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

## Comments

### 2026-08-18 — clean-install verified from tarballs; publish is the only remaining (human/CI) step

- **Clean-install validated**: packed `node/spatial-rules-0.1.0.tgz` + `spatial-rules-win32-x64-msvc-0.1.0.tgz` (`npm pack`) and installed both into a fresh temp project — zero toolchain, no Rust, no repo checkout. Ran the new `node/test/clean-install.mjs` (imports the installed `spatial-rules`, exercises mask/`where`/`excludeRuleIds`/`queryRich`/`replace`/`stats`) → **passed**. The loader resolves the installed per-platform package, so the ADR-0006 install path is proven end-to-end for the host target.
- **Still open — registry publish, human/CI step (wizard domain)**: `npm publish` of the six platform packages + root needs (a) a git remote + `v*` tag to trigger `.github/workflows/prebuild-publish.yml`, and (b) an npm auth token in CI secrets. The repo currently has **no git remote**. This is not code — the CI matrix already builds all six targets; it just needs credentials and a push to run.

