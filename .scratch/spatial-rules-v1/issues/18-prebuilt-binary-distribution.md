# Prebuilt binary distribution pipeline

Type: task
Status: open
Blocked by: 16

## Question

Phase 10 — publish prebuilt native binaries via npm per ADR-0006 (execution on the map):

- `@napi-rs/cli` per-platform packages for `linux-x64-gnu`, `linux-x64-musl`, `linux-arm64-gnu`, `linux-arm64-musl`, `win32-x64-msvc` (+ optional `win32-arm64-msvc`).
- CI matrix builds all targets; root package `optionalDependencies`; `npm install @scope/spatial-rules` is zero-toolchain.
- Verify install on a clean machine without Rust.

The package installs and loads on each supported target.
