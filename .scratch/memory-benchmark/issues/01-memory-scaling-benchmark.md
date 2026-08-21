# 01 — Memory scaling and lifecycle benchmark

Type: task
Status: ready-for-agent
Blocked by: None — can start immediately

Origin: 2026-08-21 roadmap (P0), distilled from the memory-benchmarking
brainstorm. Extends the peak-RSS method recorded in `docs/benchmarks.md`
§Memory (architecture-hardening issue 09) from a single container baseline to
a scaling and lifecycle picture.

## Question / what to build

A reproducible memory benchmark that answers: how does memory scale with
rules and geometry complexity, and does anything leak across the ruleset
lifecycle? No behavior change; measurement code + documented results only.

## Acceptance criteria

- [ ] Build vs steady-state vs query-time memory measured separately:
      peak RSS during index construction, resident footprint after input is
      dropped, and allocation behavior under repeated queries
- [ ] Scaling table across rule counts (1k / 10k / 100k) × vertices per
      polygon (10 / 100 / 1k): index bytes, bytes/rule, bytes/vertex —
      establishes whether memory tracks rule count or coordinate count
- [ ] Lifecycle check including repeated atomic ruleset replacement
      (ADR-0007 swap path) — detects retention across publishes, exercising
      the per-thread prepared-geometry cache (ADR-0010)
- [ ] Process-level RSS used as ground truth (not JS heap alone); method
      consistent with the existing `VmHWM` approach in `docs/benchmarks.md`
- [ ] Results recorded in `docs/benchmarks.md` §Memory with the generator
      script checked in alongside the existing benchmarks

## Notes

The headline publishable metrics are **memory per million vertices** and
**queries/sec per GB of RAM** — the numbers someone needs when asking whether
a national zoning dataset fits a 256 MB container.
