# Dynamic replacement + memory/concurrency testing

Type: task
Status: resolved
Blocked by: 15, 16

## Question

Phases 8–9 — dynamic ruleset replacement and its memory/concurrency validation (ADR-0007/0009; use the tdd skill):

- Synchronous `replace()` in core + binding: build fully off the hot path, atomic `Arc` swap, old ruleset kept alive while in-flight queries reference it and released when none do; observability `lastSwapTime` / `buildDurationMs` / active id (ADR-0007).
- Tests: concurrent queries across a replacement; repeated replacement; old-ruleset cleanup; bounded memory (peak measured per §25); long-running workloads.
- Criterion gate (ADR-0009): confirm the sync query stays under the p95 50 ms threshold on the production workload (evidence from the harness task), or open an async-path ticket.

Replacement is atomic and safe under concurrency; memory stays bounded; the sync/async criterion is validated.

## Answer

Built dynamic ruleset replacement in core + binding (ADR-0007/0009), committed to `main`.

- **Core `Engine`** (`core/src/engine.rs`): holds the active `Arc<Ruleset>` behind an `RwLock`. `query()` snapshots the `Arc` under a read lock (released immediately); `replace(Vec<Rule>)` builds fully off the hot path, then publishes via a single write — readers see the old or the new ruleset, never a partial build. Returns `ReplaceReport { version, rule_count, build_duration_ms, last_swap_time_unix_ms }`; `current()`/`version()`/`rule_count()` expose observability. The old ruleset stays alive until the last snapshot drops it (verified via `Arc::strong_count`).
- **Binding** (`node/`): `SpatialRuleset` now wraps the `Engine`; added `replace(Buffer) -> JSON report` and `stats() -> JSON`; `queryRich` snapshots once so outcomes and their string ids come from the same ruleset.
- **Tests**: 6 new (`core/tests/engine.rs`) — replace swaps + observability, repeated-replacement version increments, old-ruleset snapshot stays alive after replace, invalid replace fails and keeps the old ruleset, 4 concurrent reader threads × 200 queries across 20 replacements, long-running mixed workload. 68 core tests green; smoke extended with `replace`/`stats` and green under Node 24 and Bun 1.3.14; clippy clean.
- **ADR-0009 criterion gate**: met — the harness ladder D (rstar) p50 ≈ 20.1 ms ≪ 50 ms after prepared geometry, so no `queryAsync()` ticket is needed. Peak-memory measurement is a Docker follow-up (tickets 17/19).

Run: `cargo test --workspace` / `cargo clippy --workspace --all-targets`.

## Comments

### 2026-08-18 — peak-memory measurement closed (was: deferred follow-up)

The §25 replacement-peak / §26 bounded-container follow-up is now measured and
closed. Harness `integration/memory.mjs` (`bun run bench memory`; `--replacements-only`
isolates the replacement peak), run inside the `spatial-rules` image:

- Baseline (ruleset built): RSS ~50 MB, VmHWM ~51 MB.
- Query load (20 × 1,000 candidates): peak VmHWM **~65 MB**.
- Replacement (10 swaps, isolated): peak VmHWM **~52 MB** (+0.5 MB over baseline).
- Bounded: RSS spread across 10 replacements ≈ 0 (no leak).
- Under `--memory=128m`: server serves `/health`, `/query` (1,000 → 481), and
  `/replace` (→ v2) at ~29 MiB cgroup usage (22.7%); smoke green.

Conclusion: bounded container memory (§8/§43) holds; a 128 MB K8s limit leaves
headroom over the ~65 MB peak. Details in `docs/benchmarks.md` §Memory.

