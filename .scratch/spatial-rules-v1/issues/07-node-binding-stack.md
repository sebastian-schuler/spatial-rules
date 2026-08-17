# Node binding stack and native binary packaging

Type: grilling
Status: resolved

## Question

Decide the Node/Bun integration and distribution stack (§42 items 6 and 15; §21, §26–§27):

1. **Binding technology** — napi-rs vs neon vs raw Node-API (§21); stability, Bun compatibility, buffer/byte-oriented input support (§23), async work support (§28).
2. **Packaging** — prebuilt native binaries for `linux-x64` and `linux-arm64` with the correct glibc/musl split for the supported Docker images (§26); npm distribution so users don't install Rust (§27).
3. **Bun compatibility testing** — how Bun compatibility is tested explicitly, not assumed (§21).

Suggested session shape: dispatch a research subagent first for primary-source facts (napi-rs/neon current state, Bun Node-API support, prebuild options), then grill the user on the decision with a recommendation. Locked decision becomes an ADR in `docs/adr/`.

## Answer

Locked (grilling 2026-08-17, recommendations accepted):

- **Binding:** napi-rs (`napi` 3.x + `napi-derive` + `napi-build`), `features = ["napi8"]` — matches the Node-API-8 lock and the runtime matrix (Node 22/24/26 tested; Bun best-effort).
- **Packaging:** napi-rs per-platform `optionalDependencies` packages for `linux-x64-gnu`, `linux-x64-musl`, `linux-arm64-gnu`, `linux-arm64-musl` — zero-toolchain `npm install`, native glibc/musl split. **Windows works too:** add `win32-x64-msvc` (dev + CI) and optionally `win32-arm64-msvc`; napi-rs builds/tests on Windows MSVC, so local dev on a Windows machine is supported.
- **Input/output surface:** byte hot path — `query(bufferOfGeoJSON, query) -> Uint8Array mask`; rich object API for per-candidate outcomes (ADR-0004).
- **Bun compatibility:** explicit Bun smoke test in CI, non-blocking (best-effort, consistent with the runtime matrix).
- **Async:** deferred to the Sync vs async ticket; when added, `#[napi] async fn` (off-thread).

Asset: [ADR-0006](../../../docs/adr/0006-node-binding-napi-rs.md).
