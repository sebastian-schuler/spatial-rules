# Benchmark ladder additions: resolution, withinDistance, temporal, aggregation cells

Type: task
Status: resolved

## Question

The criterion ladder (`benchmarks/`, `docs/benchmarks.md`) measures only the
**match mask** path. The surfaces that shipped since (P1 resolution, P2
temporal + `withinDistance`, aggregation) all scoped "benchmark ladder
additions (separate concern)" out of their tickets — there is no performance
evidence for them yet, and the README's headline is performance.

Add benchmark cells for:

- **resolution** — `resolve` / `resolve_mask`: the ordered applicable-set
  gather, winner, and first-provider-wins values cost per batch.
- **withinDistance** — the bounding-circle pre-filter + haversine exact
  confirm throughput on a geofencing workload.
- **temporal** — the per-rule `$activeAt` window scan cost over an `at`-bearing
  query.
- **aggregation** — the rich-path aggregate over the applicable set (count /
  numeric / coverage) for a batch.

Each cell should compare against a sensible baseline (e.g. the same workload
through `query()`/`query_mask`, or the existing mask cell) and the results
must be recorded in `docs/benchmarks.md` following the existing per-cell
format. Re-run the existing cells to confirm no regression — the mask hot path
must be byte-identical (the resolve/mask/aggregate paths are additive).

Run: `cargo test --workspace`, `cargo clippy --workspace --all-targets`, and the
benchmark ladder (`bun run bench ...`) — green before commit.

## Comments

> *Deferred as "separate concern" by the P1/P2/aggregation tickets.*

> *Resolved 2026-08-24 (commit `d447f24`): ladder cells added in
> `benchmarks/benches/ladder.rs` (`batch_resolve`, `batch_within_distance`,
> `batch_temporal`, `batch_aggregation`) with mask/rich-path baselines;
> dataset fixtures `point_candidates`/`rules_with_windows` with tests; results
> recorded in `docs/benchmarks.md` (existing B–F cells re-run, no regression).
> Headline: resolution & `$activeAt` ~free, `withinDistance` ~21× the point
> mask, aggregation coverage ~13× the rich path.*

## Agent Brief

**Category:** enhancement
**Summary:** Extend the criterion benchmark ladder and `docs/benchmarks.md` with cells for resolution, `withinDistance`, temporal, and aggregation.

**Current behavior:** The ladder benchmarks the match-mask path only.

**Desired behavior:** Cells for the shipped surfaces, with recorded results and no regression in the existing cells.

**Key interfaces:** The benchmark harness (`benchmarks/`, `bench.mjs`, `benchmarks.json`), the `~1k×30` production workload, and the release-mode ruleset build.

**Acceptance criteria:**
- [ ] A cell for resolution (resolve + resolve_mask)
- [ ] A cell for `withinDistance` on a geofencing workload
- [ ] A cell for a temporal (`at` + `$activeAt`) query
- [ ] A cell for aggregation over the applicable set
- [ ] Results recorded in `docs/benchmarks.md`; existing cells re-run with no regression
- [ ] `cargo test --workspace`, clippy, and the ladder green

**Out of scope:**
- Changing the mask hot path
- turf cross-checks for the new surfaces (no turf oracle for resolution/distance/time)