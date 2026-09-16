# 09 — Measure peak container memory in Docker

Type: task
Status: resolved
Blocked by: None — can start immediately

Origin: 2026-08-19 architecture review; closes a loose end from ticket 17 (bounded container memory, Initial-plan §43).

## What to build

Record a reproducible peak-memory baseline for the container. The "bounded container memory" requirement is part of the Definition of Done, but peak RSS in the Docker container was deferred when the integration app landed (tickets 17/19) and is still unmeasured. Run the load harness in the container and record peak RSS against the documented bound, including the contribution of the per-thread prepared-geometry cache (ADR-0010). Produce a measurement method and a recorded baseline in the docs; no behavior change.

## Acceptance criteria

- [x] A reproducible peak-RSS measurement method for the container (load harness in Docker), documented
- [x] A recorded peak-RSS baseline in `docs/benchmarks.md` against the documented memory bound
- [x] The per-thread prepared-geometry cache's memory contribution is reported (or noted as deferred to the geo 0.34 ticket)
- [x] Baseline is reproducible across image rebuilds with the pinned Bun tag (ticket 06)

## Answer

Measured 2026-09-16 on the rebuilt integration image (`oven/bun:1.3.14`), via
`docs/benchmarks.md` §HTTP serving memory. The method: run the load harness
against a `--memory=128m` container, read PID 1's `VmHWM` **and** the cgroup's
`memory.peak` (both — they disagree, see below).

The deferred re-measurement corrected the ticket's own expectation. The
in-process memory harness peaks at 65 MiB, but **HTTP serving peaks at ~138–149
MiB `VmHWM` / ~119 MiB cgroup** under sustained load (30 rules × 1,000
candidates, `--concurrency=25`) — ~2.2× higher, driven by per-request parse +
body-buffering churn, not by concurrency (a `c=1` sweep still reaches ~126 MiB).
The engine **does** fit 128 MB (`memory.events` all zero, `VmHWM` flat 25 s→60 s,
RSS trimming — no leak), but at 82–93% of the cap with ~9 MiB headroom, not the
"~67 MB, comfortable" the docs previously implied. Recommendation: size
containers to 192–256 MB off the **cgroup** peak.

The turf comparison was measured the same way against the committed
`benchmarks/js/turf-server.mjs` (`benchmarks/turf.Dockerfile`), byte-identical
masks: turf idles at 102.7 MiB, peaks at 198.6 MiB uncapped, and is
**OOM-killed (exit 137) at the 128 MB cap** — at 5.3× lower throughput.

The per-thread prepared-geometry cache contribution remains deferred to the geo
0.34 upgrade (post-v1 ticket 05).
