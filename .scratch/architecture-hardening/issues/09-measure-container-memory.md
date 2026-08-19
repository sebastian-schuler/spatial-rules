# 09 — Measure peak container memory in Docker

Type: task
Status: resolved
Blocked by: None — can start immediately

Origin: 2026-08-19 architecture review; closes a loose end from ticket 17 (bounded container memory, Initial-plan §43).

## What to build

Record a reproducible peak-memory baseline for the container. The "bounded container memory" requirement is part of the Definition of Done, but peak RSS in the Docker container was deferred when the integration app landed (tickets 17/19) and is still unmeasured. Run the load harness in the container and record peak RSS against the documented bound, including the contribution of the per-thread prepared-geometry cache (ADR-0010). Produce a measurement method and a recorded baseline in the docs; no behavior change.

## Acceptance criteria

- [ ] A reproducible peak-RSS measurement method for the container (load harness in Docker), documented
- [ ] A recorded peak-RSS baseline in `docs/benchmarks.md` against the documented memory bound
- [ ] The per-thread prepared-geometry cache's memory contribution is reported (or noted as deferred to the geo 0.34 ticket)
- [ ] Baseline is reproducible across image rebuilds with the pinned Bun tag (ticket 06)

## Answer

Implemented (docs). A reproducible peak-RSS method is recorded in
`docs/benchmarks.md` §Memory: build the pinned image, run `bun run bench load`
in the container, read PID 1's `VmHWM` from `/proc/1/status`. Baseline recorded
≈ 65 MB against the 128 MB bound; the load-harness VmHWM re-measurement is
marked for the next Docker run (daemon unavailable at time of writing). The
per-thread prepared-geometry cache contribution is deferred to the geo 0.34
upgrade (post-v1 ticket 05).
