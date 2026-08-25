# 01 — Memory scaling and lifecycle benchmark

Type: task
Status: resolved
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

- [x] Build vs steady-state vs query-time memory measured separately:
      peak RSS during index construction, resident footprint after input is
      dropped, and allocation behavior under repeated queries
- [x] Scaling table across rule counts (1k / 10k / 100k) × vertices per
      polygon (10 / 100 / 1k): index bytes, bytes/rule, bytes/vertex —
      establishes whether memory tracks rule count or coordinate count
- [x] Lifecycle check including repeated atomic ruleset replacement
      (ADR-0007 swap path) — detects retention across publishes, exercising
      the per-thread prepared-geometry cache (ADR-0010)
- [x] Process-level RSS used as ground truth (not JS heap alone); method
      consistent with the existing `VmHWM` approach in `docs/benchmarks.md`
- [x] Results recorded in `docs/benchmarks.md` §Memory with the generator
      script checked in alongside the existing benchmarks

## Notes

The headline publishable metrics are **memory per million vertices** and
**queries/sec per GB of RAM** — the numbers someone needs when asking whether
a national zoning dataset fits a 256 MB container.

## Answer

Implemented (measurement code + docs, no behavior change).

## Comments

> Superseded in part by ticket 02 (lazy prepared geometries): the "Serving
> footprint" numbers in the serving-footprint paragraph below reflect the
> eager cache measured by this ticket. Ticket 02 replaced it with per-rule
> lazy preparation — serving after the first query is now ~ruleset + a small
> margin (see `docs/benchmarks.md` §Memory, 2026-08-23).

- **Harness**: `benchmarks/src/memory_scaling.rs` (library, unit-tested),
  `benchmarks/src/rss.rs` (process-level RSS/VmHWM/commit via
  `/proc/self/status` on Linux, `GetProcessMemoryInfo` on Windows), and the
  `memory_scaling` binary (`--cell`, `--rules × --vertices` cross product, or
  explicit `--cells=...`). Wired into `bun run bench memory-scale`
  (`bench.mjs` + `benchmarks.json`). Each cell runs in a fresh child process
  so peaks measure that cell alone.
- **Bounded by default**: strict validation is quadratic in per-ring vertex
  count, so the full 3×3 cross product is intractable (the `100k×1000` cell
  builds for ~8 min). The default grid is an explicit cell list that completes
  in ~5–8 min, and the aggregate harness caps each cell's replacement count to
  a ~120 s wall-time budget (`capped_replacements`), with live progress so a
  long cell never looks stuck.
- **Results recorded in `docs/benchmarks.md` §Memory**: 7-cell scaling table
  (build, ruleset steady-state delta, serving footprint after the first query,
  bytes/rule, query rate) + findings. Headline: the ruleset tracks **rule
  count, not coordinate count** (~1.2–2.7 kB/rule steady; 100k rules ≈
  118–260 MiB of ruleset), and the 50-swap probe proves **no per-replacement
  leak** (RSS plateaus flat at 270.7 MiB; the `bounded: false` verdicts on
  Windows reflect allocator arena warmup, not a leak). Queries/sec per GB of
  RAM reported as the sizing metric. **Serving footprint** measured too: the
  per-thread prepared-geometry cache (ADR-0010) adds ~2.4/15/142 kB per rule
  at 10/100/1,000 vertices on top of the ruleset — ~359 MiB (100k×10) to
  ~1.8 GiB (100k×100) resident after the first query, the geo 0.34 deferral
  (post-v1 ticket 05). **turf comparison** (`bun run bench memory-turf`): the
  engine's ruleset is ~2–5× smaller than the turf baseline's pre-parsed form
  at typical shapes, and turf beats only the engine's *serving* footprint
  (i.e. the prepared cache is the outlier, not the data).
- **Caveats**: the `10000×1000`/`100000×1000` corners are excluded from the
  default grid (hours per build). The serving-footprint paragraph above
  reflects the **eager** cache this ticket measured; ticket 02 made serving
  lazy (see its answer), and ticket 03 re-verified the whole picture on Linux —
  including resolving the "no leak claim either way" caveat: 50-swap probes at
  the big cells show a **bounded sawtooth** (glibc trim cycles), not a leak
  (`docs/benchmarks.md` §Memory).
