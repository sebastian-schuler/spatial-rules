# Opt-in queryAsync: off-main-thread query

Type: task
Status: resolved

## Answer

Implemented in `5ce241d` (`feat(node): opt-in off-main-thread queryAsync`): `queryAsync` runs off the JS thread via the libuv threadpool; sync `query()` is byte-for-byte unchanged and remains the default (ADR-0009 latency gate not triggered).

## Question

Add an **opt-in** asynchronous query to the napi surface, off the JS thread. Sync `query()` stays the default and is byte-for-byte unchanged.

**Why (measured, 2026-08-19):** the sync napi call blocks the event loop. Load test (`bun run bench load`, `benchmarks/js/server-bench.mjs`, `docs/benchmarks.md` §3b) shows a single-process CPU ceiling of ~165 rps (raw bytes) / ~130 rps (JSON) on 1,000×30, with `/health` latency ≈ query latency under load (event loop fully consumed). The original ADR-0009 latency gate (sync p95 > 50 ms) did **not** trigger — p95 ≈ 32 ms over HTTP — so this is the **throughput / event-loop-headroom axis**, not the latency axis ADR-0009 judged. This ticket adds the opt-in lever and amends ADR-0009 to record it; it does NOT flip the API to async.

## Design

- **Surface:** `#[napi]` async `queryAsync(candidates: Buffer, query: String) -> Promise<Uint8Array>` on `SpatialRuleset` (`node/src/lib.rs`), mirroring `query()`. Same `SR_*` error model, surfaced as Promise rejection (same code/message as the sync throw). `queryRichAsync` is out of scope here (add later if the rich path needs it).
- **Threading:** napi-rs async (`#[napi] async fn` / `Task`) runs the parse + query on **libuv's threadpool** (default 4 threads, `UV_THREADPOOL_SIZE`). Do not set the pool size in the library — it's a global knob and the caller's concern.
- **Buffer handling:** the candidate `Buffer` cannot be moved across threads — the task must **copy the candidate bytes** (or use a `Ref`). Expect one extra memcpy of the payload per async query; document it.
- **Engine is already thread-safe** (`RwLock<Arc<Ruleset>>` snapshot under read lock, ADR-0007; concurrency-tested in `core/tests/engine.rs`) — no core changes expected.
- **Prepared-geometry cache (ADR-0010)** is `thread_local!` keyed by `Ruleset.id` — each threadpool thread lazily builds its own clone on first async query. Bounded (one clone per thread per ruleset), but note the per-thread first-query latency.
- **Threadpool contention:** async queries share libuv's pool with fs/DNS/crypto/zlib. Document that a high async-query load can starve other pool users (and vice versa).

## Tests

- Node smoke (`node/test/smoke.mjs` or `test/`): `queryAsync` returns the **same mask** as `query` on identical inputs; Promise rejects with the same `SR_*` code as sync for invalid input; concurrent `queryAsync` calls across a `replace()` produce consistent results (snapshot semantics, ADR-0007); Bun smoke green.
- Extend `bench load` (`benchmarks/js/server-bench.mjs`) or add a small concurrent-async test: event-loop responsiveness (`/health`) must stay low while async queries are in flight, unlike the sync path.
- Sync path untouched: existing tests + ladder stay green.

## Docs

- Amend **ADR-0009** to record the opt-in `queryAsync` and the throughput/headroom rationale (the latency gate did not trigger; this is the off-main-thread lever, opt-in by design).
- Note the async costs in docs/benchmarks.md §3b or the README: per-query dispatch + promise overhead, buffer memcpy, threadpool contention, N thread-local prepared caches, concurrent in-flight memory multiplier.

Run: `cargo test --workspace` / `cargo clippy --workspace --all-targets`, node + Bun smoke, `bun run bench load` — green before commit.
