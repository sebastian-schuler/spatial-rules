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
rule's geometry once per ruleset, per thread (~5 ms for all 30), then reusing
that work across every footprint and every request — a ~23× speedup on its own.
The bounding-box index, by contrast, barely helped: with 30 large zones, almost
every footprint overlaps some zone's box anyway.

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
- **Prepare cost is ~4.6 ms for 30 rules**, now paid once per thread per
  ruleset: `PreparedGeometry` is `!Send` in geo 0.33, so owned prepared
  geometries are cached in a `thread_local!` keyed by ruleset identity
  (ADR-0010) rather than rebuilt per query or stored in the shared
  `Arc<Ruleset>`.
- **ADR-0009 gate met**: sync p50 ≈ 20 ms ≪ 50 ms, so `queryAsync()` is not
  needed for v1.
- **Correctness**: the turf cross-check is green; Rust and turf agree on all
  10 predicate pairs (and on the full 1,000 × 30 matched count).

## Memory (container footprint)

Closes the deferred follow-up from tickets 17/19 — §24 "the exact memory layout
must be benchmarked" and §25 "peak memory during replacement MUST be measured
because the application runs in constrained containers". Harness:
`integration/memory.mjs` (`cd integration && bun memory.mjs`, or
`REPLACEMENTS_ONLY=1 bun memory.mjs` to isolate the replacement peak).

Measured inside the `spatial-rules` Docker image (oven/bun:1.3), 30 rules ×
1,000 candidates:

| Phase | RSS | VmHWM (peak resident) |
|---|---|---|
| Baseline (Bun + addon + ruleset built) | ~50 MB | ~51 MB |
| Query load (20 × 1,000 batches) | ~62 MB sampled | **~65 MB** |
| Replacement, isolated (10 swaps, no queries) | ~51 MB | **~52 MB** (≈ +0.5 MB over baseline) |
| Boundedness | spread across 10 replacements ≈ 0 (no leak) | — |

- **Peak resident ≈ 65 MB** on the production workload; replacement adds only
  ~0.5 MB of peak on top of baseline (the old ruleset is dropped by the atomic
  swap, and both coexist for only the ~18 ms build).
- **Bounded**: RSS does not climb across repeated replacements (first ≈ last),
  so there is no per-replacement leak.
- **Works under a hard cap**: `docker run --memory=128m --memory-swap=128m`
  serves `/health`, `/query` (1,000 → 481 matched), and `/replace` (→ v2) at
  ~29 MiB actual cgroup usage (22.7% of the cap); `integration/smoke.mjs` green.
- `VmPeak` (~132 GB) is Bun/JSC's virtual-address-space reservation, **not**
  resident memory — ignore it when sizing container limits. The number that
  matters for a K8s `limits.memory` is VmHWM (~65 MB), so a 128 MB limit leaves
  comfortable headroom.

## Commands

```bash
cargo bench -p spatial-rules-benchmarks --bench ladder       # criterion ladder
cargo run -p spatial-rules-benchmarks --bin generate_dataset  # regenerate dataset

cd benchmarks/js && npm install && npm run cross-check        # correctness vs turf
cd benchmarks/js && npm run perf                              # JS baseline vs addon

cd integration && bun memory.mjs                              # container memory (§24/§25)
REPLACEMENTS_ONLY=1 bun memory.mjs                            # isolate replacement peak

# verify under a hard cap (§26)
docker build -f integration/Dockerfile -t spatial-rules .
docker run --rm --memory=128m --memory-swap=128m -p 3000:3000 spatial-rules
node integration/smoke.mjs
```
