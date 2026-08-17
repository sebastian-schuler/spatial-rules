# 09 — Supported Runtimes: Node.js, Bun, Binding Frameworks

Researched 2026-08-13. Primary sources only; every claim cites its URL.

## 1. Node.js release lines (as of 2026-08-13)

Source: schedule JSON at https://github.com/nodejs/Release/blob/main/schedule.json;
statuses per https://nodejs.org/en/about/previous-releases.

- **v22 "Jod" — Maintenance LTS**: LTS 2024-10-29, maintenance 2025-10-21, EOL **2027-04-30**.
- **v24 "Krypton" — Active LTS**: LTS 2025-10-28, maintenance 2026-10-20, EOL **2028-04-30**.
  Latest LTS release: **v24.19.0** (nodejs.org footer).
- **v26 — Current**: initial 2026-05-05, LTS starts **2026-10-28**, EOL 2029-04-30.
  Latest release: **v26.7.0** (nodejs.org footer).
- v25 is EOL (2026-06-01); v27 enters alpha 2026-10-28.
- Only Active/Maintenance LTS is recommended for production use (nodejs.org).

## 2. Node-API version matrix

Source: https://nodejs.org/api/n-api.html#node-api-version-matrix (latest docs).

- Node-API 10: v22.14.0+, v23.6.0+ and all later versions.
- Node-API 9: v18.17.0+, v20.3.0+, v21.0.0+ and all later versions.
- Node-API 8: v12.22.0+, v14.17.0+, v15.12.0+, v16.0.0+ and all later versions.
- If `NAPI_VERSION` is unset it defaults to **8**; higher versions require opt-in.
- Node-API 9 changed versioning semantics: an addon built for 9 may need code
  changes for 10 — prefer 8 unless you need newer APIs (same source).
- Every supported line (v22+) therefore provides Node-API 8, 9, and 10.

## 3. Bun: version, Node-API claims, caveats

- Latest stable: **Bun v1.3.14**, published 2026-05-13, per
  https://github.com/oven-sh/bun/releases/latest.
- Official claim: "Bun implements this interface [Node-API] from scratch, so most
  existing Node-API extensions work with Bun out of the box." `require()` of
  `.node` files and `process.dlopen` work as in Node. https://bun.sh/docs/api/node-api
- Tracker https://github.com/oven-sh/bun/issues/158 (closed as completed
  2026-04-12): "Every `napi_*` and `node_api_*` function is implemented and
  exported." Bun runs Node.js' own test suites in CI on every commit:
  js-native-api 53/53 (100%), node-api 21/44 (47.7%) on Linux.
- Thread-safe functions & async work: implemented (`src/napi/napi.zig` — "Threadsafe
  functions, async work, event loop, handle scopes") and exercised by Bun's tests.
  Bun's own test addon asserts `napi_get_version >= 10`
  (test/napi/napi-app/standalone_tests.cpp in the Bun repo).
- Known gaps/caveats, per issue #158:
  - `napi_async_init/destroy/make_callback/open|close_callback_scope` work but do
    not emit `async_hooks` lifecycle events — not planned.
  - Bun (Linux/macOS) does not run on libuv; only a subset of `uv_*` symbols is
    exported. Addons must use `napi_get_uv_event_loop`, not `uv_default_loop()`
    (see oven-sh/bun #23192; libuv tracking #18546).
  - Finalizer/buffer ordering during `Worker.terminate()` does not fully match
    Node (tracking #15964).
  - `napi_adjust_external_memory` behavior differs from V8's.
- Bun's Node.js compatibility page reflects Node.js v23 status:
  https://bun.sh/docs/runtime/nodejs-apis
- Recent N-API fixes in Bun itself: v1.3.13 fixed LIFO finalizer ordering on exit
  (sqlite3/duckdb/kuzu/node-llama-cpp); v1.3.14 fixed
  `napi_create_external_buffer|arraybuffer` double-free edge cases.
  https://bun.com/blog/bun-v1.3.13 , https://bun.com/blog/bun-v1.3.14

## 4. Binding frameworks on Bun and prebuild platforms

### napi-rs

- "Bun native addons — Best effort. The source repository runs a latest-Bun job,
  but the test step is continue-on-error, so Bun failures do not block napi-rs
  releases. Test your actual addon before claiming support."
  https://napi.rs/docs/more/support-compatibility
- Source CI tests Node.js **22, 24, 26** across Linux/macOS/Windows; CLI requires
  Node `>=23.5.0 || ^22.13.0 || ^20.17.0`; Rust MSRV 1.88; produced addons need
  Node >= 10. Same URL + https://github.com/napi-rs/napi-rs (README).
- Template prebuild matrix (`napi new`): macOS x64/arm64; Windows MSVC
  x64/x86/arm64; Linux glibc x64/arm64/armv7; Linux musl x64/arm64; Android
  arm64/armv7; FreeBSD x64; threaded-WASI preview-1. CLI accepts more triples
  (loong64, riscv64gc, ppc64le, s390x, OpenHarmony, Windows GNU) without a
  scaffolded publish path. Same URL.
- Scaffold offers Node-API levels 1–9, default 4. Same URL.

### Neon

- "Bun (experimental): In many cases Neon modules will work in bun; however, at
  the time of this writing, some Node-API functions are not implemented"
  (links oven-sh/bun #158). https://github.com/neon-bindings/neon (README).
- "Neon actively supports all current and maintenance releases of Node"; CI
  targets Node 22, 24, 26. OS support: Windows, macOS, Linux. Same README.
- Latest release: v1.1.1 (GitHub releases page).

## 5. Recommendation

| Runtime | Version(s) | Status |
| --- | --- | --- |
| Node.js | v22.x "Jod" (Maintenance LTS) | Tested, supported |
| Node.js | v24.x "Krypton" (Active LTS) | Tested, supported (primary) |
| Node.js | v26.x (Current) | Tested, supported |
| Bun | v1.3.14 (latest stable) | Smoke-tested, **best-effort** |

- "Tested" = the addon's own test suite runs in CI on that exact runtime entry
  before each release; a Node.js failure blocks the release.
- Bun is not a blocking gate: failures are reported and tracked, not release-
  blocking (mirrors napi-rs policy above). Claim "supports Bun 1.3.14+" only
  after the smoke test passes.
- **Target Node-API version: 8** (`NAPI_VERSION 8` / napi-rs `napi8` feature).
  - Lowest needed by features: sync batch = Node-API 1; one async query path
    (Promise/`napi_create_async_work`) = Node-API 4.
  - 8 is provided by every matrix entry (Node 22+ ships 8–10; Bun's test suite
    asserts `napi_get_version >= 10`) and is Node's default, so prebuilds avoid
    the v9→v10 code-change churn documented in the Node-API version matrix.
- Ship one prebuilt binary per platform compiled with Node-API 8; it loads on all
  four matrix rows without per-major rebuilds (ABI stability per
  https://nodejs.org/api/n-api.html).
