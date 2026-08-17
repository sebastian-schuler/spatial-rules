# Supported Node.js and Bun runtime matrix

Type: research
Status: resolved

## Question

Determine the runtime version matrix the addon must support (§42 item 7; §21):

- Which Node.js versions are supported in production (exact versions per §21), and which Node-API version each target provides.
- Which Bun versions run Node-API addons reliably, with primary-source evidence for any known gaps or caveats (Bun's Node-API status, GitHub issues, release notes).
- What napi-rs and neon claim for Bun compatibility, and whether the binding choice affects the matrix.
- Recommend a concrete matrix: Node LTS/maintenance lines + exact Bun version, and what "tested" means for each entry.

Findings against primary sources (nodejs.org releases, Bun docs/release notes, napi-rs/neon docs), written to `research/09-supported-runtimes.md` and linked here.

## Answer

Full findings with per-claim sources: [research/09-supported-runtimes.md](../research/09-supported-runtimes.md).

Recommended matrix (as of 2026-08-13):

- **Node.js v22.x "Jod"** (Maintenance LTS, EOL 2027-04) — tested, supported.
- **Node.js v24.x "Krypton"** (Active LTS, EOL 2028-04) — tested, supported, primary target.
- **Node.js v26.x** (Current) — tested, supported.
- **Bun v1.3.14** (latest stable) — smoke-tested, best-effort (not a release blocker).

**Target Node-API 8**: provided by every matrix entry, avoids Node-API 9→10 code-change churn, covers the feature floor (sync batch = N-API 1; async query = N-API 4). One prebuilt binary per platform loads on all four rows. Bun caveats to respect: no async_hooks lifecycle events, use `napi_get_uv_event_loop` (not `uv_default_loop()`), and Bun is best-effort for both napi-rs and neon.
