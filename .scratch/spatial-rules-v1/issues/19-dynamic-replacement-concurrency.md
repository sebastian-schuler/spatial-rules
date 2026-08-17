# Dynamic replacement + memory/concurrency testing

Type: task
Status: open
Blocked by: 15, 16

## Question

Phases 8–9 — dynamic ruleset replacement and its memory/concurrency validation (ADR-0007/0009; use the tdd skill):

- Synchronous `replace()` in core + binding: build fully off the hot path, atomic `Arc` swap, old ruleset kept alive while in-flight queries reference it and released when none do; observability `lastSwapTime` / `buildDurationMs` / active id (ADR-0007).
- Tests: concurrent queries across a replacement; repeated replacement; old-ruleset cleanup; bounded memory (peak measured per §25); long-running workloads.
- Criterion gate (ADR-0009): confirm the sync query stays under the p95 50 ms threshold on the production workload (evidence from the harness task), or open an async-path ticket.

Replacement is atomic and safe under concurrency; memory stays bounded; the sync/async criterion is validated.
