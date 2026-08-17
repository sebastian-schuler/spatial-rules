# Benchmark dataset, harness, and reference cross-checks

Type: task
Status: open
Blocked by: 01, 02, 03

## Question

Build the measurement infrastructure the benchmark-driven decisions depend on (§31–§33):

1. **Dataset** — synthesize a representative dataset from open data (Natural Earth / OSM admin boundaries): ~30 country-scale, partly highly complex MultiPolygon rules plus ~1,000 polygon candidates per request, stored as GeoJSON.
2. **Harness** — measure p50/p95/p99 latency, throughput, steady-state and peak memory, ruleset build time, replacement time; run 100 / 1,000 / 10,000 requests (§31).
3. **Algorithm ladder** — A existing JS implementation, B Rust naive (candidate×rule), C + bbox filtering, D + spatial index, E + prepared geometries, F index+prepared (§32).
4. **turf.js cross-checks** — a correctness suite comparing predicate results against turf.js as the trusted reference (§33).

Answer records where the dataset lives, how to run the harness, and the initial numbers once the core exists. This ticket unblocks Sync vs async query and replacement API.
