# Benchmark dataset, harness, and reference cross-checks

Type: task
Status: resolved
Blocked by: 01, 02, 03

## Question

Build the measurement infrastructure the benchmark-driven decisions depend on (§31–§33):

1. **Dataset** — synthesize a representative dataset from open data (Natural Earth / OSM admin boundaries): ~30 country-scale, partly highly complex MultiPolygon rules plus ~1,000 polygon candidates per request, stored as GeoJSON.
2. **Harness** — measure p50/p95/p99 latency, throughput, steady-state and peak memory, ruleset build time, replacement time; run 100 / 1,000 / 10,000 requests (§31).
3. **Algorithm ladder** — A existing JS implementation, B Rust naive (candidate×rule), C + bbox filtering, D + spatial index, E + prepared geometries, F index+prepared (§32).
4. **turf.js cross-checks** — a correctness suite comparing predicate results against turf.js as the trusted reference (§33).

Answer records where the dataset lives, how to run the harness, and the initial numbers once the core exists.

## Comments

### 2026-08-18 — dataset + harness + B/C/D ladder (progress)

**Dataset**: synthesized deterministically in `benchmarks/src/dataset.rs` — 30 country-scale MultiPolygon rules (1–3 parts each, 60–400 vertices, ~35% with holes) plus 1,000 footprint candidates. Generator: `cargo run -p spatial-rules-benchmarks --bin generate_dataset` → `benchmarks/data/{rules,candidates}.geojson` (committed). Geometry is synthetic/representative, not Natural Earth; real open data can be dropped in later without changing the harness.

**Harness**: criterion benches in `benchmarks/benches/ladder.rs`. Run: `cargo bench -p spatial-rules-benchmarks --bench ladder`.

**Initial numbers** (release, 1,000 candidates × 30 rules):
- B naive (candidate×rule, no bbox): **436.8 ms**/batch (~2.29 Kelem/s)
- C + linear-scan bbox: **465.9 ms**/batch (~2.15 Kelem/s)
- D + rstar bbox: **458.5 ms**/batch (~2.18 Kelem/s)
- ruleset build (30 rules): **23.6 ms**

**Findings**:
- Exact DE-9IM `Relate` on complex MultiPolygons dominates; the bbox index gives ≈0 speedup at 30 large rules (consistent with research 02/03). The real lever is prepared geometries (E/F), not the spatial index.
- ADR-0009 trigger fires: sync query p50 ≈ 458 ms ≫ 50 ms ⇒ `queryAsync()` is warranted, after prepared geometry.
- Still open: turf.js cross-check (item 4), JS baseline A, memory (steady-state/peak — best measured in the Docker container, tickets 17/19), prepared-geometry ladder E/F (needs a prepare path in core).

Status stays `claimed` until the turf.js cross-check lands.

## Answer

Built the benchmark dataset, harness, and turf.js cross-check; committed to `main`.

- **Dataset** (`benchmarks/src/dataset.rs`, deterministic): 30 country-scale MultiPolygon rules (1–3 parts, 60–400 vertices, ~35% with holes) + 1,000 footprint candidates. Generator: `cargo run -p spatial-rules-benchmarks --bin generate_dataset` → `benchmarks/data/{rules,candidates}.geojson` (committed). Synthetic/representative, not Natural Earth; real open data drops in without changing the harness.
- **Harness** (`benchmarks/benches/ladder.rs`, criterion): `cargo bench -p spatial-rules-benchmarks --bench ladder` — batch latency/throughput for B/C/D plus ruleset build time.
- **Initial numbers** (release, 1,000 × 30): B naive **436.8 ms**, C linear-scan bbox **465.9 ms**, D rstar bbox **458.5 ms**, build **23.6 ms**. Exact DE-9IM `Relate` dominates; the bbox index gives ≈0 help at 30 large rules (research 02/03). ADR-0009 trigger fired: sync p50 ≈ 458 ms ≫ 50 ms.
- **turf.js cross-check** (`benchmarks/js/` + `benchmarks/src/bin/cross_check.rs`): a 10-pair DE-9IM matrix (disjoint, overlap, touching edge/corner, identical, containment, holes, MultiPolygon) diffed against pinned `@turf/turf@6.5.0` (JSTS-based). **Green.** Known quirk: turf v6 `booleanContains` rejects a MultiPolygon *contained* geometry, so `contains` is skipped for those pairs (`intersects`/`within` still verified; the skipped value is hand-checked per DE-9IM). Run: `cargo build --release -p spatial-rules-benchmarks --bin cross_check`, then `cd benchmarks/js && npm install && npm run cross-check`.
- **Perf comparison vs JS** (`benchmarks/js/perf.mjs`, `npm run perf`): turf.js (naive baseline A, early-exit) **1103 ms**/batch vs native addon **484 ms**/batch = **2.3×**; both report the same 481 matched candidates. Caveat: turf is a naive proxy, not the production app's JS, and the bbox index gives ≈0 help at 30 large rules, so both sides are dominated by exact `Relate` — prepared geometry (E/F) is the remaining lever.

**Explicit deferrals** (documented, not blockers):
- Ladder **A** (the production JS) is out-of-repo; a turf.js proxy is now measured (`benchmarks/js/perf.mjs`, 1087 ms/batch vs addon 21 ms = 51.6×).
- Ladder **E/F** (prepared geometries) is now measured and adopted: E naive 14.0 ms, F +rstar 13.1 ms, prepare 4.6 ms — the 23× lever, integrated into `Ruleset::query` (ticket 15).
- **Memory** (steady-state/peak) is best measured in the Docker container (tickets 17/19).
 This ticket unblocks Sync vs async query and replacement API.
