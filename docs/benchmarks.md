# Benchmarks

Measurement infrastructure for the spatial rules engine, driven by the
benchmark-dependent decisions in `docs/Initial-plan.md` §31–§33 (ADR-0002,
ADR-0008, ADR-0009).

## Summary

**One number to remember: ~20 ms per request.** Evaluating 1,000 candidate
footprints against 30 rules returns its match mask in about 20 ms — roughly
**52× faster** than the same check written in JavaScript with turf.js
(≈1.1 s per batch).

**Why it got fast.** The cost was dominated by the exact geometry check ("does
this footprint touch this zone?"). The fix that mattered was **preparing** each
rule's geometry once per request (~5 ms for all 30), then reusing that work
across every footprint — a ~23× speedup on its own. The bounding-box index, by
contrast, barely helped: with 30 large zones, almost every footprint overlaps
some zone's box anyway.

**Practical impact.** 20 ms is well under the 50 ms ceiling for blocking the
event loop, so v1 ships a simple synchronous API and needs no async path.

| Workload (1,000 candidates × 30 rules) | Time per batch |
|---|---|
| turf.js (JavaScript baseline) | ~1 090 ms |
| Rust — before prepared geometries | ~470 ms |
| Rust — after prepared geometries | **~13–20 ms** |

## Dataset

- **Source**: `benchmarks/src/dataset.rs` — deterministic (seeded LCG, no
  network), representative geometry: 30 country-scale MultiPolygon rules
  (1–3 parts, 60–400 vertices, ~35% with holes) + 1,000 footprint candidates.
- **Artifacts**: `benchmarks/data/{rules,candidates}.geojson` (committed) and
  `benchmarks/data/cross_check.json` (10 named predicate pairs for the turf
  cross-check).
- **Regenerate**: `cargo run -p spatial-rules-benchmarks --bin generate_dataset`
- Geometry is synthetic/representative, not Natural Earth; real open data can
  be dropped in without changing the harness.

## Harnesses

### 1. Algorithm ladder (criterion)
`benchmarks/benches/ladder.rs` — `cargo bench -p spatial-rules-benchmarks --bench ladder`

Ladder (§32), each on the same 1,000 × 30 batch:

| Bench | What it measures |
|---|---|
| `B_naive_candidate_times_rule` | every candidate × every rule, exact DE-9IM only (no index, unprepared) |
| `C_linear_scan_bbox` | + bounding-box filtering (linear envelope scan) |
| `D_rstar_bbox` | + spatial index (`rstar` bulk-load, the default) |
| `E_prepared_naive` | + prepared geometries (no bbox) |
| `F_prepared_rstar_bbox` | + spatial index + prepared geometries |
| `ruleset_build/build_30_rules` | ruleset construction |
| `prepare/prepare_30_rules` | per-worker `PreparedGeometry` preparation |

### 2. JS performance baseline (turf.js vs addon)
`benchmarks/js/perf.mjs` — `cd benchmarks/js && npm run perf`

Times the native addon's `query(Buffer) → Uint8Array` hot path against a naive
turf.js `booleanIntersects` baseline A on the same workload; both report the
same matched-candidate count.

### 3. turf.js cross-check (correctness, ADR-0008)
`benchmarks/js/cross_check.mjs` + `benchmarks/src/bin/cross_check.rs` —
`cd benchmarks/js && npm run cross-check`

Diffs the 10-pair DE-9IM matrix against pinned `@turf/turf@6.5.0` (JSTS-based).
Known quirk: turf v6 `booleanContains` rejects a MultiPolygon *contained*
geometry, so `contains` is skipped for those two pairs (`intersects`/`within`
are still verified).

## Results (release profile, 1,000 candidates × 30 rules)

| Bench | Time/batch | Throughput |
|---|---|---|
| B naive (unprepared) | 471.9 ms | 2.1 Kelem/s |
| C linear-scan bbox | 19.6 ms | 51 Kelem/s |
| D rstar bbox | 20.1 ms | 50 Kelem/s |
| E prepared (naive) | 14.0 ms | 71 Kelem/s |
| F prepared + rstar | 13.1 ms | 76 Kelem/s |
| ruleset build (30 rules) | 22.2 ms | — |
| prepare 30 rules | 4.6 ms | — |

vs turf.js (`perf.mjs`): turf 1 087 ms vs addon **21 ms = 51.6×** (both report
481 matched candidates). The addon figure includes the Buffer→parse→mask
round-trip.

## Findings

- **Prepared geometry is the lever.** It cuts the core query ~23× (B → C/D);
  the bbox index alone gives ≈0 help at 30 large rules (B ≈ C ≈ D when
  unprepared).
- **Prepare cost is negligible** (~4.6 ms for 30 rules), so per-call
  preparation is the production path. `PreparedGeometry` is `!Send` in geo
  0.33, so it is built once per `query()` call rather than stored in the
  shared `Arc<Ruleset>`.
- **ADR-0009 gate met**: sync p50 ≈ 20 ms ≪ 50 ms, so `queryAsync()` is not
  needed for v1.
- **Correctness**: the turf cross-check is green; Rust and turf agree on all
  10 predicate pairs (and on the full 1,000 × 30 matched count).

## Commands

```bash
cargo bench -p spatial-rules-benchmarks --bench ladder       # criterion ladder
cargo run -p spatial-rules-benchmarks --bin generate_dataset  # regenerate dataset

cd benchmarks/js && npm install && npm run cross-check        # correctness vs turf
cd benchmarks/js && npm run perf                              # JS baseline vs addon
```
