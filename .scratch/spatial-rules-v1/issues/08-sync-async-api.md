# Sync vs async query and replacement API

Type: grilling
Status: resolved
Blocked by: 12

## Question

Decide the query and replacement API concurrency model (§42 items 8 and 14; §28–§29):

1. **Sync vs async query** — whether `query(...)` blocks the event loop or offloads to a worker thread; the spec requires a benchmark-driven call using the harness from Benchmark dataset, harness, and reference cross-checks on realistic geometries.
2. **Ruleset replacement API** — synchronous or asynchronous `replace(...)`, and whether build progress is surfaced (see Ruleset build cancellation).
3. **Concurrency guarantees** — read-mostly immutable ruleset, active queries finishing on the old ruleset, atomic publication (§29); decide the exact mechanism (e.g. `Arc` swap) only after the harness produces numbers.

Locked decision becomes an ADR in `docs/adr/`.

## Answer

Locked (grilling 2026-08-17, provisional per option (b); recommendations accepted):

- **Sync-first default:** v1 ships synchronous `query()` and `replace()`; no event-loop offload in the initial surface.
- **Async trigger criterion:** add `queryAsync()` (off-thread, `#[napi] async fn`, ADR-0006) if the harness task's p95 sync-query latency on the ~1,000-candidate production workload exceeds **50 ms**. The criterion is locked now; the Benchmark harness task validates it.
- **Replacement API:** synchronous `replace()` returning ADR-0007 observability (`lastSwapTime`, `buildDurationMs`, active id/count); no async replace in v1.

Asset: [ADR-0009](../../../docs/adr/0009-sync-first-api.md).
