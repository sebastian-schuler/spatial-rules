# Benchmarks

Measurement infrastructure for the spatial rules engine, driven by the
benchmark-dependent decisions in `docs/Initial-plan.md` §31–§33 (ADR-0002,
ADR-0008, ADR-0009).

A **rule** is a geometry-bearing feature with queryable properties; a
**candidate** is a geometry evaluated against the rules. The full glossary is in
`CONTEXT.md`.

## Summary

**One number to remember: ~15 ms per request.** Evaluating 1,000 candidate
footprints against 30 rules returns its match mask in about 15 ms (18.5 ms
through the native addon) — roughly **60× faster** than the same check written
in JavaScript with turf.js (≈1.1 s per batch).

**Why it got fast.** The cost was dominated by the exact geometry check ("does
this footprint touch this zone?"). The fix that mattered was **preparing** each
rule's geometry once per ruleset, per thread (~5 ms for all 30), then reusing
that work across every footprint and every request — a ~34× speedup on its own.
The bounding-box index, by contrast, barely helped: with 30 large zones, almost
every footprint overlaps some zone's box anyway.

**Practical impact.** ~15 ms is well under the 50 ms ceiling for blocking the
event loop, so v1 ships a simple synchronous API and needs no async path.

| Workload (1,000 candidates × 30 rules) | Time per batch |
|---|---|
| turf.js (JavaScript baseline) | ~1 090 ms |
| Rust — before prepared geometries | ~470 ms |
| Rust — after prepared geometries | **~14–15 ms** |

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
| B naive (unprepared) | 502.2 ms | 2.0 Kelem/s |
| C linear-scan bbox | 14.6 ms | 68 Kelem/s |
| D rstar bbox | 15.5 ms | 65 Kelem/s |
| E prepared (naive) | 14.1 ms | 71 Kelem/s |
| F prepared + rstar | 13.8 ms | 72 Kelem/s |
| ruleset build (30 rules) | 24.0 ms | — |
| prepare 30 rules | 5.3 ms | — |

vs turf.js (`perf.mjs`): turf 1 111 ms vs addon **18.5 ms = 60.0×** (both report
481 matched candidates). The addon figure includes the Buffer→parse→mask
round-trip.

## Findings

- **Prepared geometry is the lever.** It cuts the core query ~34× (B → C/D);
  the bbox index alone gives ≈0 help at 30 large rules (B ≈ C ≈ D when
  unprepared).
- **Prepare cost is ~5.3 ms for 30 rules**, now paid once per thread per
  ruleset: `PreparedGeometry` is `!Send` in geo 0.33, so owned prepared
  geometries are cached in a `thread_local!` keyed by ruleset identity
  (ADR-0010) rather than rebuilt per query or stored in the shared
  `Arc<Ruleset>`.
- **The cache closes the indexed/prepared gap.** Once preparation is no longer
  per-call, the index-backed benches fall to the hand-rolled prepared level:
  C 19.6 → 14.6 ms and D 20.1 → 15.5 ms (this table's earlier run).
- **ADR-0009 gate met**: sync p50 ≈ 18.5 ms ≪ 50 ms, so `queryAsync()` is not
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

## Limitations suite — why turf doesn't scale

Five harnesses demonstrate turf.js's limits and why the engine exists
(`cd benchmarks/js && npm install`, then `npm run scale` / `npm run fair` /
`npm run http` / `npm run complex` / `npm run crossover` — the scripts run
under `bun`, which also auto-loads `benchmarks/js/.env`). All workloads assert
turf and the addon agree on the matched count before timing.

#### How the measurements are taken (and why it's fair to turf)

- **The addon is timed for the full call a user makes**: `ruleset.query(buffer,
  queryJson)` — GeoJSON parse + napi + index + relate + mask, every query.
- **The turf side is timed only for the relate loop**, and is given the benefit
  of every cheap optimization: geometries pre-parsed into JS objects, per-rule
  and per-candidate bboxes precomputed *outside* the timed region, and a warmup
  call so JIT is hot. It never re-serializes or re-parses.
- Both sides are measured **min-of-N reps** (N = 3) to damp scheduler/GC noise,
  and the matched count is asserted equal *before* timing — so the numbers are
  never comparing a wrong answer.
- Because the addon still carries its parse + FFI cost while turf is handed
  pre-parsed data, **a turf win would be real, and an addon win is conservative**
  — the handicap runs against the addon, not against turf.
- The turf baseline escalates across harnesses: naive scan (no index) in
  `scale.mjs` → scan + bbox fast-reject in `complex.mjs`/`crossover.mjs` →
  `rbush` + turf in `fair.mjs`. `crossover.mjs` uses the middle one; `fair.mjs`
  uses the strongest hand-rolled JS answer.
- Known turf property, not an artifact: `booleanIntersects` flattens a
  MultiPolygon and relates each part without a per-part bbox short-circuit, so
  island countries (many parts) genuinely cost turf more.

### 1. Scaling sweep (`scale.mjs`)

Complex blob rules (120–300 vertices, ~35% with holes) laid out on a grid, so a
bbox index filters to ~1 rule per candidate while a naive scan touches every
rule. turf runs the naive scan; the addon runs the full mask query.

| rules × candidates | turf (ms) | addon (ms) | speedup |
|---|---|---|---|
| 30 × 100 | 55.8 | 0.53 | 105× |
| 30 × 1,000 | 469.5 | 4.52 | 104× |
| 30 × 10,000 | 5,277 | 50.8 | 104× |
| 100 × 1,000 | 1,328 | 5.16 | 257× |
| 300 × 1,000 | 5,220 | 5.55 | **941×** |

- turf scales O(candidates × rules): **5.2 s** at 300 × 1,000 — ~100× over the
  50 ms event-loop budget. The addon stays ~5.5 ms: the bbox index filters and
  prepared geometries are cached per thread (ADR-0010).
- At fixed 30 rules both scale with candidates (the addon re-parses GeoJSON per
  call), but turf is still ~104× slower at every point.

### 2. Fair competitor (`fair.mjs`)

The strongest pure-JS answer — an `rbush` bbox index + turf relate — vs the
addon, same full-mask output, 300 rules × 1,000 candidates:

| variant | time |
|---|---|
| naive turf (scan) | 5,289 ms |
| rbush + turf (indexed) | 15.7 ms |
| native addon | **5.58 ms** |

The rbush index makes JS 337× faster — but the engine is still ~2.8× faster, and
the JS had to hand-roll the index the engine ships by default.

### 3. Production query over HTTP (`http-bench.mjs`)

The full query shape — `intersects` + `where{classification}` +
`excludeRuleIds` — served by the Bun + Express addon over HTTP, vs the
equivalent hand-rolled turf in-process (a lower bound: turf has no
`where`/exclusion API, and a turf endpoint would add its own overhead).

- addon over HTTP (10 requests): **22.3 ms/request** — under the 50 ms budget.
- optimized in-process turf: 181.9 ms/batch — 3.6× over budget.
- **8×**, and the addon serves the complete query (parse + `where` + exclusions
  + mask) where turf needs application code around it.

### 4. Complexity & metadata stress (`complex.mjs`)

Two modes: synthetic "coastline" rules, and any real boundary file via
`RULES_FILE`. In real-data mode candidates are derived from the file's own
geometry and the `where` clause is picked from a shared property on the first
feature; invalid rules are dropped so both sides agree (see below).

**Synthetic** (defaults: 3 rules × 3 parts × 5,000 vertices/ring + 40 fields ≈
1.8 MB, 47k vertices):

| phase | addon | turf (scan + bbox) |
|---|---|---|
| build (parse + validate + index) | ~932 ms | — |
| first query (prepared-geometry build) | ~63 ms | — |
| query, steady state (20 candidates) | **0.91 ms** | 6.7 ms |
| query + where | **0.85 ms** | 3.1 ms |

**Real boundary** — `RULES_FILE=countries.geojson` (Natural Earth 10 m
`ne_10m_admin_0_countries`, public domain): 13,287,234 bytes (12.67 MiB),
258 countries, 546k vertices, 168 properties.

| phase | addon | turf (scan + bbox) |
|---|---|---|
| build (parse + validate + index) | ~17.6 s | — |
| first query (prepared-geometry build) | ~125 ms | — |
| query, steady state (20 candidates) | **4.8 ms** | 0.4 ms |
| query + where | 4.8 ms | 0.4 ms |

- The addon's query is **independent of rule complexity**: a 47k-vertex
  synthetic ruleset and a 546k-vertex real-world ruleset both query in
  milliseconds, because prepared geometries (ADR-0010) are built once and the
  R*-tree filters the 258 countries to the ~1 that overlaps each candidate.
- The one-time build (0.9 s synthetic, **17.6 s** real) is dominated by strict
  geometry validation; the 168-property index adds no measurable query cost.
- The addon's ~5 ms real-data query is its per-call floor (GeoJSON re-parse +
  napi + index) — at 20 candidates clustered on one country, turf's bbox
  fast-reject has almost no relate work, so it lands below that floor. A true
  naive turf scan (relate every candidate × every country, ~5,100 relates)
  takes minutes; the bbox fast-reject is the hand-rolled baseline an `rbush`
  index would replace. The opposite corner — candidates spread across the
  whole map, so every one does a real relate — is the crossover sweep in §5.
- Natural Earth has one country with a self-intersecting exterior; the engine's
  strict validation (ADR-0005) rejects it and the harness drops it (257 rules
  loaded, "1 invalid dropped" in the output).
- `core/tests/complex.rs` asserts correctness at scale (2,000-vertex rule,
  40 properties, holes, indexed `where`); larger sizes stay in this benchmark,
  where the release addon avoids debug-mode validation cost.

  ```bash
  cd benchmarks/js
  curl -L -o countries.geojson https://raw.githubusercontent.com/nvkelso/natural-earth-vector/master/geojson/ne_10m_admin_0_countries.geojson
  # bun auto-loads .env — set the file there for a persistent default:
  printf 'RULES_FILE="countries.geojson"\n' > .env   # PowerShell: Set-Content -Value 'RULES_FILE="countries.geojson"' -Path .env
  bun complex.mjs

  # or keep just one country (bun, no jq needed) for a focused run:
  bun -e "const fs=require('fs');const g=JSON.parse(fs.readFileSync('countries.geojson','utf8'));fs.writeFileSync('deu.geojson',JSON.stringify({type:'FeatureCollection',features:g.features.filter(x=>x.properties.ADMIN==='Germany')}))"
  RULES_FILE=deu.geojson bun complex.mjs
  ```

### 5. Crossover sweep (`crossover.mjs`)

At how many candidates does the native binding beat a hand-rolled turf scan?
Sweeps candidates 20 → 5,000 (min-of-3) on the real `countries.geojson`
ruleset, with candidates scattered across the whole map (sampled from every
country's boundary, so each one does a real relate). Full addon query vs turf
scan + bbox fast-reject, matched counts asserted each step.

| candidates | addon (ms) | turf (ms) | speedup |
|---|---|---|---|
| 20 | 8.8 | 42.7 | 4.9× |
| 200 | 80.6 | 553.0 | 6.9× |
| 1,000 | 392.9 | 2,868.6 | 7.3× |
| 5,000 | 1,903.3 | 14,392.0 | 7.6× |

- With candidates spread over the map both sides do roughly the same relates;
  the ~5–8× and growing gap is dominated by prepared geometry (ADR-0010)
  beating turf's JSTS relate per call. The R*-tree contributes little here (only
  257 rules) — its payoff shows in the rule-count axis below.
- The addon wins from the smallest size tested (20); its ~5 ms per-query floor
  only matters below ~20 candidates. §4's 0.4 ms turf figure is the opposite
  corner — 20 candidates clustered on one country, where turf has almost no
  relate work and the addon's floor dominates.
- Falls back to a synthetic 500-rule grid when no `RULES_FILE` is set;
  `RULES=… SIZES=… REPS=…` tune it.

**Second axis — rule count** (`MODE=rules bun crossover.mjs`, synthetic grid,
fixed 1,000 candidates):

| rules | addon (ms) | turf (ms) | speedup |
|---|---|---|---|
| 500 | 3.5 | 11.8 | 3.4× |
| 1,000 | 4.0 | 13.6 | 3.4× |
| 2,000 | 4.7 | 17.5 | 3.8× |
| 5,000 | 4.9 | 21.9 | 4.5× |

- The R*-tree keeps the addon ~flat (3.5 → 4.9 ms over 10× rules) while turf's
  scan + bbox reject grows with the scan (11.8 → 21.9 ms) — the index payoff at
  moderate rule counts; the naive-scan version at 30 → 300 rules is §1 (up to
  941×).
- Grid candidates land on ~35% of rules' centre holes, so matched ≈ 650 here —
  both sides still agree, and the relate work is unchanged.

## Commands

```bash
cargo bench -p spatial-rules-benchmarks --bench ladder       # criterion ladder
cargo run -p spatial-rules-benchmarks --bin generate_dataset  # regenerate dataset

cd benchmarks/js && npm install && npm run cross-check        # correctness vs turf
cd benchmarks/js && npm run perf                              # JS baseline vs addon
cd benchmarks/js && npm run scale                             # scaling sweep (§limitations)
cd benchmarks/js && npm run fair                              # rbush+turf fair competitor
cd benchmarks/js && npm run http                              # full query over HTTP
cd benchmarks/js && npm run complex                           # complexity & metadata stress
cd benchmarks/js && npm run crossover                         # candidate-count crossover sweep

cd integration && bun memory.mjs                              # container memory (§24/§25)
REPLACEMENTS_ONLY=1 bun memory.mjs                            # isolate replacement peak

# verify under a hard cap (§26)
docker build -f integration/Dockerfile -t spatial-rules .
docker run --rm --memory=128m --memory-swap=128m -p 3000:3000 spatial-rules
node integration/smoke.mjs
```
