# 01 — Deepen the Candidate: validated, envelope-carrying intake

Type: task
Status: resolved
Blocked by: None — can start immediately

Origin: 2026-08-19 architecture review, candidate 1 (top recommendation).

## What to build

Make a candidate carry its classification instead of re-deriving it in the query hot path. Today a candidate is a bare geometry: the query engine re-runs full OGC validity checking plus envelope computation on every candidate on every query. Deepen the candidate so it arrives at the query engine already classified — valid/invalid, with its bounding envelope precomputed at intake. The query hot path then reads the cached envelope and runs pure spatial index + DE-9IM relate; per-candidate `invalid` outcomes (unsupported type, non-finite coordinate, invalid geometry, no bounding rectangle) still surface per query and never fail the batch (ADR-0005 semantics preserved).

**Scope the win honestly.** The Node/Bun surface re-parses candidates from a fresh buffer on every request, so per-request cost is work-neutral here — the win is locality (validation concentrates at the intake seam) plus a real throughput gain for callers who hold a candidate batch and re-query it (the Rust core API today; any future "accept candidates once" capability). A "hold candidates across queries" Node capability is a separate design question (grill/ADR before ticketing), not part of this ticket.

This is a shared-type change (a candidate is built at ingestion and consumed by the engine, the Node binding, the benchmark dataset, and tests). Sequence it as expand–contract: introduce the classified intake form alongside the existing construction path so nothing breaks, migrate callers in batches, then contract — keeping the whole workspace green at every step.

## Acceptance criteria

- [ ] A candidate batch parsed once can be re-queried without re-running OGC validation; a test proves validation runs once per intake (e.g. a probe/counter), not once per query
- [ ] The query hot path uses a precomputed envelope instead of recomputing it per candidate
- [ ] Per-candidate `invalid` outcomes (unsupported type, non-finite coordinate, invalid geometry, no bounding rectangle) still surface per query and never fail the batch (ADR-0005)
- [ ] Behavior is unchanged: `query` / `query_mask` / `queryRich` produce identical results; full core test suite, node + Bun smoke, criterion ladder, and turf cross-check all green
- [ ] Load-harness gate: `bun run bench load` HTTP p95 and rps not regressed
- [ ] ADR-0005 amended to note validation + envelope are computed at intake

## Answer

Implemented (commit `0ae1576`). `Candidate` now carries a `CandidateClass`
(`Valid { envelope }` / `Invalid { reason }`) computed once at intake via
`Candidate::new` / `candidate_from_feature`; the query hot path reads the cached
envelope or the recorded invalid reason instead of re-running OGC validation per
query. A probe test (`validation_runs_once_at_intake_not_per_query`) counts
`classify_candidate` calls and asserts they fire once at intake, never per
query. ADR-0005 amended. Core tests, node + Bun smoke, and the cross-check are
green.
