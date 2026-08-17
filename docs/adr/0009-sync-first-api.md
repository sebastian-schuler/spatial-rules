# Query/replacement API: sync-first with a benchmark trigger

v1 ships synchronous `query()` and `replace()` — simplest and correct for the workload; the request path never touches a partial build (`docs/Initial-plan.md` §37). An asynchronous `queryAsync()` (off the JS thread via `#[napi] async fn`, ADR-0006) is added only if the benchmark harness shows the sync query's p95 latency exceeding 50 ms on the ~1,000-candidate production workload (§28). `replace()` returns the ADR-0007 observability (`lastSwapTime`, `buildDurationMs`, active ruleset id/count).
