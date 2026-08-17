# 07 — Node binding + packaging: primary-source research (2026-08-17)

Supports the Node binding stack ticket. Sources: GitHub READMEs/CHANGELOGs/CLI source/templates (napi-rs, neon). Note: napi.rs / docs.rs / crates.io web fetches were unavailable this session; version claims come from GitHub sources and are flagged where unverified.

## napi-rs (2026-08)

- Versions: `napi` 3.12.1 (2026-08-10), `napi-derive` 3.6.3, `napi-build` 2.4.1, `napi-sys` 3.3.0, `@napi-rs/cli` 3.8.6. https://github.com/napi-rs/napi-rs/blob/main/crates/napi/CHANGELOG.md
- MSRV 1.88; Linux support covers gnu + musl for x64/arm64; "builds add-ons… without involving node-gyp". https://github.com/napi-rs/napi-rs/blob/main/crates/napi/README.md
- Node-API levels are Cargo features `napi1`–`napi10` (default `napi4`); set `features = ["napi8"]`; CLI `napi new --min-node-api`. https://github.com/napi-rs/napi-rs/blob/main/cli/src/api/new.ts
- Per-platform prebuilds: `napi build --platform` + `napi prepublish` → one npm package per target (`@scope/spatial-rules-linux-x64-gnu`), stamped `os`/`cpu`/`libc`; root package gets `optionalDependencies` map; generated loader detects musl via `ldd`/`process.report`. Result: `npm install` = zero-toolchain. https://github.com/napi-rs/napi-rs/blob/main/cli/src/api/pre-publish.ts, .../create-npm-dirs.ts, .../templates/js-binding.ts
- Async: `#[napi] async fn` → `execute_tokio_future_with_finalize_callback` on a Tokio runtime in an additional thread (`tokio_rt`), or pluggable `AsyncRuntime` (Rayon-backed). Off the JS thread, resolves a promise. https://github.com/napi-rs/napi-rs/blob/main/crates/backend/src/codegen/fn.rs
- Thread-safe functions + `Env::spawn`/`Task` → `AsyncWorkPromise` on libuv pool. https://github.com/napi-rs/napi-rs/blob/main/crates/napi/src/threadsafe_function.rs
- Byte input: full `Buffer`/`Uint8Array` support (`JsBuffer`, `create_buffer_with_data`). https://github.com/napi-rs/napi-rs/blob/main/crates/napi/src/bindgen_runtime/js_values/buffer.rs
- Bun: project runs its own test suite under Bun (best-effort, not release-blocking). https://github.com/napi-rs/napi-rs/blob/main/examples/napi/__tests__/bun-test.js

## neon (2026-08)

- Stable 1.1.x (1.1.1 N-API<5 hotfix); 1.2.0-alpha.0 in dev. https://github.com/neon-bindings/neon/blob/main/RELEASES.md
- Macro-based: `#[neon::export]`, `#[neon::main]`, `#[neon::class]`. https://github.com/neon-bindings/neon/blob/main/crates/neon/src/macros.rs
- Node support: all current+maintenance (22/24/26); **Bun explicitly experimental** — tolerates missing Node-API symbols (warns at load, panics only on use). https://github.com/neon-bindings/neon/blob/main/README.md
- Node-API ceiling napi-8 (features napi-1/4/5/6/8). https://github.com/neon-bindings/neon/blob/main/crates/neon/src/sys/bindings/functions.rs
- Async: `cx.task(..).promise(..)` on Node's worker pool; `Channel`/`Deferred` (napi-4); tokio/futures features. https://github.com/neon-bindings/neon/blob/main/crates/neon/src/macros.rs
- Byte input: `JsBuffer`, `JsArrayBuffer`, `JsTypedArray<T>`. https://github.com/neon-bindings/neon/blob/main/crates/neon/src/types_impl/buffer/types.rs
- Packaging: `@neon-rs/cli` + `@neon-rs/load` per-platform packages (`@<org>/<pkg>-<platform>`); generated GitHub release workflow builds/publishes every platform; zero-toolchain. https://github.com/neon-bindings/neon/blob/main/pkgs/create-neon/README.md
- MSRV stable 1.65+. https://github.com/neon-bindings/neon/blob/main/README.md

## Raw Node-API

- `napi-sys` (napi-rs monorepo) 3.3.0: hand-rolled `extern "C"` for napi1–10 + experimental; async work, buffer/typed-array, promises; `libloading`. https://github.com/napi-rs/napi-rs/blob/main/crates/sys/src/functions.rs
- Higher-level `node-api` crate (repo napi-rs/node-api): unverified this session; hand-write async work, byte reads, module registration, and the whole packaging pipeline yourself — much more manual.

## Prebuild distribution patterns

- **napi-rs per-platform `optionalDependencies`** → npm picks one per host; `libc` field splits glibc/musl; loader picks gnu vs musl. Zero-toolchain.
- **neon per-platform packages + `@neon-rs/load`** → same per-platform npm publishing. Zero-toolchain.
- **prebuildify** → single package embedding per-platform `.node` files; you roll glibc/musl naming.
- **node-gyp-build + prebuilt tarballs** → runtime pick or compile fallback (consumers can hit a toolchain).
- **GitHub Releases + postinstall download** → requires postinstall network fetch; less robust behind proxies.
- For linux-x64/arm64 with glibc **and** musl, napi-rs's `libc`-tagged optionalDependencies and neon's per-platform packages are the only two that encode the ABI split natively in npm metadata.

## Async off-thread execution

- napi-rs `#[napi] async fn` → Tokio on an additional thread (or Rayon-backed custom runtime); independent of libuv; best-effort on Bun.
- Manual `napi_create_async_work` (also napi-rs `Env::spawn`, neon `cx.task`) → libuv thread pool, which Bun implements — slightly more Bun-friendly, but hand-rolled.
- Either fine for a sync batch now; keep async optional and test under Bun before promising it.

## Workspace integration

- napi-rs: binding crate `crate-type = ["cdylib"]` + `napi`/`napi-derive` + build-dep `napi-build` with `napi_build::setup()`; core is a normal path dependency (repo's own `examples/napi-shared` proves it).
- neon: generated template emits a Cargo workspace with the addon as a `cdylib` member; a sibling core crate is another member + path dependency.

## Options

- **A. napi-rs (recommended default).** Most active (3.12.1); turnkey per-platform `optionalDependencies` packaging with native glibc/musl split for linux-x64/arm64; `napi8` feature matches the Node-API-8 lock; `#[napi] async fn` off-thread; Buffer input + generated `.d.ts`; zero-toolchain install. Cons: Bun best-effort; async backend is a design choice.
- **B. neon.** Clean `#[neon::export]`; per-platform prebuilds via `@neon-rs/cli` + `@neon-rs/load`; tolerates Bun's missing N-API symbols; JsBuffer/typed-array; async via libuv. Cons: N-API ceiling 8 (fine); no TS typegen; slower cadence (1.1.x / 1.2.0-alpha).
- **C. raw node-api / napi-sys.** Minimal deps, full control; hand-write every binding + entire prebuild/glibc-musl pipeline — highest effort for the same result.

**Recommended default:** Option A — napi-rs, `features = ["napi8"]`, build targets `x86_64-unknown-linux-gnu`, `x86_64-unknown-linux-musl`, `aarch64-unknown-linux-gnu`, `aarch64-unknown-linux-musl`; sync batch as a plain `#[napi]` fn taking a `Buffer`; async later via `#[napi] async fn`.
