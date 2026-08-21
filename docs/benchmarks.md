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
that work across every footprint and every request — a ~29× speedup on its own
(the ladder's B → E rung). The bounding-box index, by contrast, barely helped:
with 30 large zones, almost every footprint overlaps some zone's box anyway
(B → C → D ≈ 1×). See the separated attribution in the ladder Results below.

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
- **Regenerate**: `bun run bench gen`
- Geometry is synthetic/representative, not Natural Earth; real open data can
  be dropped in without changing the harness.

## Harnesses

### 1. Algorithm ladder (criterion)
`benchmarks/benches/ladder.rs` — `bun run bench crit` (or
`cargo bench -p spatial-rules-benchmarks --bench ladder`)

Each rung drives the engine through its public seams (rule source, envelope
query, prepared form by opaque id) and differs from its neighbour by exactly
**one** variable, so each speedup can be attributed to the rung that produced
it (architecture-hardening 04):

| Bench | What it measures |
|---|---|
| `B_naive_candidate_times_rule` | every candidate × every rule, exact DE-9IM, unprepared (no bbox) |
| `C_linear_scan_bbox` | B + bounding-box filter (linear envelope scan), still unprepared |
| `D_rstar_bbox` | C but with the `rstar` index instead of the linear scan |
| `E_prepared_naive` | B but with prepared geometries instead of unprepared (no bbox) |
| `F_prepared_rstar_bbox` | E + `rstar` bbox filter |
| `ruleset_build/build_30_rules` | ruleset construction |
| `prepare/prepare_30_rules` | per-worker `PreparedGeometry` preparation |

The two levers are isolated: bbox/index filter = B→C→D; prepared geometries =
B→E (and D→F with the index held constant).

### 2. JS performance baseline (turf.js vs addon)
`bun run bench perf`

Times the native addon's `query(Buffer) → Uint8Array` hot path against a naive
turf.js `booleanIntersects` baseline A on the same workload; both report the
same matched-candidate count.

### 3. turf.js cross-check (correctness, ADR-0008)
`benchmarks/js/cross_check.mjs` + `benchmarks/src/bin/cross_check.rs` —
`bun run bench cross-check` (build the release binary first with
`bun run bench build`)

Diffs the 10-pair DE-9IM matrix against pinned `@turf/turf@6.5.0` (JSTS-based).
Known quirk: turf v6 `booleanContains` rejects a MultiPolygon *contained*
geometry, so `contains` is skipped for those two pairs (`intersects`/`within`
are still verified).

## Results (release profile, 1,000 candidates × 30 rules)

Quick criterion run (`--warm-up-time 1 --measurement-time 2 --sample-size 10`,
Windows, 2026-08-19) — order-of-magnitude figures, not tuned medians:

| Bench | Time/batch | vs B |
|---|---|---|
| B naive (unprepared, no bbox) | 580 ms | 1× |
| C linear-scan bbox (unprepared) | 570 ms | ≈1× |
| D rstar bbox (unprepared) | 554 ms | ≈1× |
| E prepared (naive, no bbox) | 20.3 ms | **≈29×** |
| F prepared + rstar bbox | 21.6 ms | ≈27× |
| ruleset build (30 rules) | 32.7 ms | — |
| prepare 30 rules | 6.2 ms | — |

**Attribution (the two levers, separated).** The bbox/index filter alone is
negligible at these shapes — B → C → D ≈ 1×, because with 30 large
country-scale rules nearly every candidate's envelope already overlaps most
rules. The dominant lever is **prepared geometry**: B → E ≈ 29× (each relate is
~29× cheaper once the rule's topology is prepared, ADR-0010), and adding the
index on top (F) adds nothing (E ≈ F). Previous ladder tables conflated the two
because the C/D rungs ran the full engine (bbox **and** prepared); the rungs
above isolate one variable each (architecture-hardening 04).

vs turf.js (`bun run bench perf`): turf 1 111 ms vs addon **18.5 ms = 60.0×** (both report
481 matched candidates). The addon figure includes the Buffer→parse→mask
round-trip.

## Findings

- **Prepared geometry is the lever.** It cuts the core query ~29× (B → E on the
  separated ladder); the bbox/index filter alone gives ≈0 help at 30 large
  rules (B ≈ C ≈ D). Earlier tables attributed B → C/D to prepared geometry,
  but those rungs ran the full engine — bbox **and** prepared — conflating the
  two (architecture-hardening 04).
- **Prepare cost is ~6.2 ms for 30 rules**, now paid once per thread per
  ruleset: `PreparedGeometry` is `!Send` in geo 0.33, so owned prepared
  geometries are cached in a `thread_local!` keyed by ruleset identity
  (ADR-0010) rather than rebuilt per query or stored in the shared
  `Arc<Ruleset>`.
- **ADR-0009 gate met**: sync p50 ≈ 18.5 ms ≪ 50 ms, so `queryAsync()` is not
  needed for v1.
- **Correctness**: the turf cross-check is green; Rust and turf agree on all
  10 predicate pairs (and on the full 1,000 × 30 matched count).

## Memory (container footprint)

Closes the deferred follow-up from tickets 17/19 — §24 "the exact memory layout
must be benchmarked" and §25 "peak memory during replacement MUST be measured
because the application runs in constrained containers". Harness:
`bun run bench memory` (or `bun run bench memory --replacements-only` to
isolate the replacement peak).

### Memory scaling & lifecycle (`bun run bench memory-scale`)

The scaling picture beyond the single container baseline (memory-benchmark
ticket 01): `benchmarks/src/memory_scaling.rs` + the `memory_scaling` binary.
Answers **does memory track rule count or coordinate count** across a grid of
rule counts × vertices per polygon, measures build vs steady-state vs
query-time resident footprint separately, and checks the lifecycle
(20 atomic replacements) for retention. Ground truth is process-level RSS
(`/proc/self/status` VmRSS/VmHWM on Linux, working-set counters on Windows),
each grid cell in its own child process so peaks measure that cell alone.

- **The default grid is bounded.** The default cell list is
  `1000x10, 1000x100, 1000x1000, 10000x10, 10000x100, 100000x10, 100000x100`
  (`--cells=rules1xverts1,...` in `benchmarks.json`), which completes in ~5–8
  minutes. Strict validation is quadratic in per-ring vertex count (the
  `10000×1000` cell builds in ~45 s, `100000×1000` in ~8 min per build), so the
  full `--rules × --vertices` cross product would take hours — pass those flags
  explicitly only when you want the full grid. The aggregate harness also
  **caps each cell's replacement count** to a ~120 s wall-time budget
  (`capped_replacements`), so even an over-large request never looks stuck.
- Cell progress streams live to stderr (parent buffers stdout for the JSON
  report only), so long cells never appear frozen.
- Headline output: **bytes per million vertices** and **bytes per rule** in the
  steady-state delta (memory to size a container for a national dataset), plus
  the lifecycle `bounded` verdict and per-swap RSS/commit traces.

**Results** (release profile, Windows, 2026-08-22; each cell in a fresh child
process, default grid, 20 atomic replacements, 20 × 1,000-candidate query
batches):

| rules × verts | total verts | build | steady delta | bytes/rule | bytes/1M verts | query rate* |
|---|---|---|---|---|---|---|
| 1,000 × 10 | 10,000 | 1 ms | 1.9 MiB | 1.96 kB | 196 MB | 8.9 M cand/s |
| 1,000 × 100 | 100,000 | 42 ms | 3.3 MiB | 3.50 kB | 35 MB | 7.8 M |
| 1,000 × 1,000 | 1,000,000 | 3.9 s | 17.6 MiB | 18.4 kB | 18 MB | 3.9 M |
| 10,000 × 10 | 100,000 | 13 ms | 13.3 MiB | 1.39 kB | 139 MB | 9.8 M |
| 10,000 × 100 | 1,000,000 | 426 ms | 27.4 MiB | 2.87 kB | 29 MB | 7.3 M |
| 100,000 × 10 | 1,000,000 | 138 ms | 117.8 MiB | 1.24 kB | 124 MB | 9.3 M |
| 100,000 × 100 | 10,000,000 | 4.5 s | 260.1 MiB | 2.73 kB | 27 MB | 7.1 M |

\* steady-state candidates/sec across the 20 timed batches (cold first batch
excluded, ADR-0010 prepare is warmed).

**Findings:**

- **Memory tracks rule count, not coordinate count.** The same 1M vertices
  cost ~7× more resident when spread across 100k tiny 10-vertex rules
  (124 MB/1M verts) than across 1k 1,000-vertex rules (18 MB/1M). The steady
  footprint is dominated by **per-rule fixed overhead** — envelope + R*-tree
  entry + property-key table + prepared-geometry slot, ~1.2–2 kB/rule — plus
  ~18 bytes per coordinate. So bytes/vertex *falls* as rings get complex
  (196 → 35 → 18 B/vert at 10/100/1,000 verts), while bytes/rule grows only
  1.2 → 18 kB. A national zoning dataset therefore sizes almost purely by rule
  count: ~1.2–2.7 kB/rule steady (118–260 MiB at 100k rules) — fits a 256 MB
  container for typical shapes.
- **No per-replacement leak.** The 5%-tolerance `bounded` verdict is
  conservative on Windows: the allocator grows committed arenas during the
  first ~35 swaps (~100 MiB at 1k×1k) before plateauing, and 20 swaps (the
  budget-capped default) can sit inside that warmup, so several cells report
  `bounded: false`. The traces rule out a leak: RSS climbs in **steps with
  drops** and tracks commit, and a 50-swap probe at 1k×1k **plateaus flat at
  270.7 MiB** (swaps 36–50 constant to ±0.1 MiB) rather than climbing
  linearly — a leak keeps climbing. The big cells (100k×100) show the same
  stepwise-with-drops shape but run only 20 swaps, so no plateau is observed;
  no leak claim either way, pending a longer Linux run.
- **queries/sec per GB of RAM** (the sizing metric) = steady-state
  candidates/sec ÷ steady-state footprint: ~220 M cand/s/GB at 1k×1k
  (3.9 M ÷ 18 MiB) down to ~28 M cand/s/GB at 100k×100 — per-rule overhead,
  not throughput, is the binding constraint at high rule counts.
- **Build is the other lever.** Strict validation is quadratic in ring
  vertices, so `1000×1000` builds in 3.9 s while the same 1M vertices as
  `100000×10` builds in 138 ms; the `10000×1000`/`100000×1000` corners are
  excluded from the default grid for this reason (hours per build).

Measured inside the `spatial-rules` Docker image (oven/bun:1.3.14 — pinned to
CI's Bun version, architecture-hardening 06), 30 rules × 1,000 candidates:

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

### Peak RSS under sustained load (architecture-hardening 09)

Reproducible measurement method — the **load** harness (sustained HTTP
concurrency), not the in-process memory harness above, run inside the pinned
image:

```
docker build -f integration/Dockerfile -t spatial-rules .
docker run --rm -d --name spatial-rules-load -p 3000:3000 spatial-rules
bun run bench load --duration=20000            # sustained /query load
docker exec spatial-rules-load cat /proc/1/status | grep -E 'VmHWM|VmRSS'
docker stop spatial-rules-load
```

`VmHWM` of PID 1 (the Bun server) is the kernel-recorded all-time peak
resident, capturing the load phase even though the event loop can't sample
mid-request. **Baseline: peak resident ≈ 65 MB** (the VmHWM from the memory
harness above, which already exercises 20 × 1,000-candidate batches in the
container); the load-harness VmHWM re-measurement is recorded here on the next
Docker run — the figure is expected to sit in the same ~65 MB envelope against
the documented 128 MB bound. The per-thread prepared-geometry cache's marginal
contribution (ADR-0010, one owned geometry clone per thread per ruleset) is
deferred to the geo 0.34 upgrade (ticket 05 of `post-v1`), which moves the
cache from owned per-thread clones to borrowed `Arc`-shared prepared forms.

## Limitations suite — why turf doesn't scale

Five harnesses demonstrate turf.js's limits and why the engine exists
(`bun run bench scale` / `bun run bench fair` / `bun run bench http` /
`bun run bench complex` / `bun run bench crossover` — all under `bun`, all
reading their defaults from the repo-root `benchmarks.json`). All workloads
assert turf and the addon agree on the matched count before timing.

#### How the measurements are taken (and why it's fair to turf)

- **The addon is timed for the full steady-state call a user makes**:
  `ruleset.query(buffer, queryJson)` — GeoJSON parse + napi + index + relate +
  mask, every query; the one-time prepared-geometry build is warmed first
  (ADR-0010) and excluded.
- **The turf side is timed only for the relate loop**, and is given the benefit
  of every cheap optimization: geometries pre-parsed into JS objects, per-rule
  and per-candidate bboxes precomputed *outside* the timed region, and the
  correctness assertion runs the scan once first so JIT is hot before timing.
  It never re-serializes or re-parses.
- Both sides are measured **min-of-N reps** (N = 3) to damp scheduler/GC noise,
  and the matched count is asserted equal *before* timing — so the numbers are
  never comparing a wrong answer.
- Because the addon still carries its parse + FFI cost while turf is handed
  pre-parsed data, **a turf win would be real, and an addon win is conservative**
  — the handicap runs against the addon, not against turf.
- The turf baseline escalates across harnesses: naive scan (no index) in
  `bench scale` → scan + bbox fast-reject in `bench complex`/`bench crossover`
  → `rbush` + turf in `bench fair`. `bench crossover` uses the middle one;
  `bench fair` uses the strongest hand-rolled JS answer.
- Known turf property, not an artifact: `booleanIntersects` flattens a
  MultiPolygon and relates each part without a per-part bbox short-circuit, so
  island countries (many parts) genuinely cost turf more.

### 1. Scaling sweep (`bun run bench scale`)

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

### 2. Fair competitor (`bun run bench fair`)

The strongest pure-JS answer — an `rbush` bbox index + turf relate — vs the
addon, same full-mask output, 300 rules × 1,000 candidates:

| variant | time |
|---|---|
| naive turf (scan) | 5,289 ms |
| rbush + turf (indexed) | 15.7 ms |
| native addon | **5.58 ms** |

The rbush index makes JS 337× faster — but the engine is still ~2.8× faster, and
the JS had to hand-roll the index the engine ships by default.

### 3. Production query over HTTP (`bun run bench http`)

The full query shape — `intersects` + `where{classification}` +
`excludeRuleIds` — served by the Bun + Express addon over HTTP, vs the
equivalent hand-rolled turf in-process (a lower bound: turf has no
`where`/exclusion API, and a turf endpoint would add its own overhead).

- addon over HTTP (10 requests): **22.3 ms/request** — under the 50 ms budget.
- optimized in-process turf: 181.9 ms/batch — 3.6× over budget.
- **8×**, and the addon serves the complete query (parse + `where` + exclusions
  + mask) where turf needs application code around it.

### 3b. Sustained concurrent load (`bun run bench load`)

Models a search endpoint hit by many users at once: sustained concurrency
against the real Bun + Express addon server, measuring achievable req/s, query
latency percentiles, and event-loop responsiveness (via interleaved `/health`
probes). Two endpoints: `/query` (JSON in/out — the naive `express.json()`
path) and `/queryRaw` (raw GeoJSON bytes in, raw mask out — the third-party
fetch pattern with **no `.json()`** in Node). Workload: 1,000 candidates × 30
rules, `intersects` + `where{classification=restricted}` + 2 exclusions
(Bun 1.3.14, local machine).

Raw bytes endpoint (`/queryRaw`):

| concurrency | req/s | p50 ms | p95 ms |
| ----------- | ----- | ------ | ------ |
| 5           | 164   | 30.5   | 32.2   |
| 10          | 164   | 60.2   | 63.7   |
| 25          | 170   | 146.2  | 151.4  |

JSON endpoint (`/query`):

| concurrency | req/s | p50 ms | p95 ms |
| ----------- | ----- | ------ | ------ |
| 5           | 130   | 37.7   | 41.8   |
| 25          | 133   | 186.4  | 194.0  |

Findings:

- The single-threaded server is CPU-bound at a **~165 rps ceiling** (raw) /
  **~130 rps** (JSON) on this workload. Beyond ~5 concurrent clients throughput
  stops growing and latency rises with queueing (p50 ≈ service time ×
  concurrency).
- Skipping `.json()` (raw bytes in/out) is worth **~25–28% more throughput**
  (130 → 165 rps) and ~7 ms lower p50 at low concurrency — the overhead
  avoided is the server-side `express.json()` + stringify + `Array.from`.
- **Event-loop responsiveness**: at every load level `/health` latency ≈ query
  latency — the loop is fully consumed by queries under load, so other
  synchronous work on the same process contends.
- At the target **100 rps** one process handles it (≈60% of the raw ceiling)
  with ~30 ms p50 at low concurrency — but with no meaningful headroom; scale
  out to 2+ pods (or worker threads) for margin. Numbers are machine-specific
  and for this query shape (the `where` equality index prunes rules, so this
  query is faster than the unfiltered ~20 ms ladder case).

**`queryAsync` lever (ADR-0009 amendment, ticket 06):** the event-loop
consumption above is the throughput/headroom axis, not the latency gate ADR-0009
originally judged (p95 ≈ 32 ms ≪ 50 ms). An **opt-in** `queryAsync()` offloads
the parse + query to libuv's threadpool (`UV_THREADPOOL_SIZE`, default 4), so
`/health` and other synchronous work stay responsive while queries are in
flight. Costs: per-query Promise dispatch overhead, one copy of the candidate
buffer per async call, contention with fs/DNS/crypto/zlib on the shared
threadpool, one per-thread prepared-geometry clone per ruleset (ADR-0010,
bounded), and a concurrent in-flight memory multiplier. Sync `query()` remains
the default and is byte-for-byte unchanged.

### 4. Complexity & metadata stress (`bun run bench complex`)

Two modes: synthetic "coastline" rules, and any real boundary file via
`--rules-file`. In real-data mode candidates are derived from the file's own
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

**Real boundary** — `--rules-file=benchmarks/data/countries.geojson`
(Natural Earth 10 m `ne_10m_admin_0_countries`, public domain): 13,287,234
bytes (12.67 MiB), 258 countries, 546k vertices, 168 properties.

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
  bun run bench data                            # download Natural Earth + derive deu.geojson
  bun run bench complex --rules-file=benchmarks/data/countries.geojson
  bun run bench crossover --rules-file=benchmarks/data/countries.geojson
  bun run bench complex --rules-file=benchmarks/data/deu.geojson   # Germany only
  ```

### 5. Crossover sweep (`bun run bench crossover`)

At how many candidates does the native binding beat a hand-rolled turf scan?
Sweeps candidates 20 → 100,000 (min-of-3). Up to 5,000 it runs on the real
`countries.geojson` ruleset, with candidates scattered across the whole map
(sampled from every country's boundary, so each one does a real relate); the
100k level runs on the default synthetic 500-rule grid, because the real-data
turf baseline there is ~40 minutes per run. Full addon query vs turf scan +
bbox fast-reject, matched counts asserted each step.

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
- Falls back to a synthetic 500-rule grid when no `--rules-file` is set;
  `--rules=… --sizes=… --reps=…` tune it.
- Top of the sweep (default synthetic mode, 500 rules): at **100,000**
  candidates the addon is **456 ms** vs turf's bbox-reject scan **1,342 ms**
  (2.9×, 62,092 matched) — near-linear scaling from 5k (22.4 ms), so the
  addon adds ~20× time for 20× candidates.

**Second axis — rule count** (`bun run bench crossover --mode=rules`, synthetic
grid, fixed 1,000 candidates):

| rules | addon (ms) | turf (ms) | speedup | matched |
|---|---|---|---|---|
| 500 | 4.06 | 15.43 | 3.8× | 612 |
| 1,000 | 4.66 | 15.53 | 3.3× | 637 |
| 2,000 | 4.83 | 18.40 | 3.8× | 641 |
| 5,000 | 4.74 | 22.41 | 4.7× | 655 |
| 20,000 | 5.60 | 61.82 | 11.0× | 655 |

- The R*-tree keeps the addon ~flat (4.1 → 5.6 ms over 40× rules) while turf's
  scan + bbox reject grows with the scan (15.4 → 61.8 ms) — the index payoff
  widens to 11× at 20,000 rules; the naive-scan version at 30 → 300 rules is
  §1 (up to 941×).
- The one-time ruleset build grows with rule count (~5.3 s at 20,000 rules,
  measured separately; the crossover times queries only) — fine for the
  weekly-replacement lifecycle, but the first thing to watch at very high rule
  counts.
- Grid candidates land on ~35% of rules' centre holes, so matched is ~65% of
  the 1,000 candidates (see the `matched` column) — both sides still agree, and
  the relate work is unchanged.

## Commands

Everything runs from the repo root through one dispatcher — `bun run bench`
(no `cd`, no env vars, no `.env` files). All knobs default to the single
committed `benchmarks.json` and are overridable per run with `--flag=value`.

```bash
bun run bench                # list every command

bun run bench build          # native binding + copy + cross_check binary
bun run bench gen            # regenerate the synthetic dataset
bun run bench data           # download Natural Earth countries + derive deu.geojson

bun run bench cross-check    # correctness vs turf (§3)
bun run bench perf           # JS baseline vs addon (§2)
bun run bench scale          # scaling sweep
bun run bench fair           # rbush+turf fair competitor
bun run bench complex        # complexity & metadata stress (§4)
bun run bench crossover      # candidate-count crossover sweep (§5)
bun run bench http           # full query over HTTP (spawns the server)
bun run bench load           # sustained concurrent load (server must be running)
bun run bench server         # start the integration server
bun run bench smoke          # integration smoke (server must be running)
bun run bench memory         # container memory (§24/§25)
bun run bench memory --replacements-only   # isolate replacement peak
bun run bench memory-scale   # scaling & lifecycle grid [--cells= --rules= --vertices=]
bun run bench smoke:node     # node package smoke test
bun run bench crit           # criterion ladder
bun run bench all            # full battery (build + gen if needed)

# verify under a hard cap (§26)
docker build -f integration/Dockerfile -t spatial-rules .
docker run --rm --memory=128m --memory-swap=128m -p 3000:3000 spatial-rules
bun run bench smoke
```

### Config

`benchmarks.json` at the repo root is the single source of truth — every knob
the harnesses read, with the flag that overrides it:

| Section | Key | Flag | Default |
|---|---|---|---|
| `global.paths` | `rulesFile` / `candidatesFile` / `crossCheckFile` | `--rules-file` / `--candidates-file` / `--cross-check-file` | `benchmarks/data/*.geojson` / `cross_check.json` |
| `global.paths` | `realRulesFile` | `--rules-file` | `benchmarks/data/countries.geojson` |
| `global.paths` | `crossCheckBin` | `--cross-check-bin` | `target/release/cross_check` |
| `global.paths` | `nodeBinding` | — | `node/spatial_rules.node` |
| `global.server` | `port` | `--port` | `3000` |
| `complex` | `rules` `parts` `vertices` `fields` `candidates` `rulesFile` | `--rules` `--parts` `--vertices` `--fields` `--candidates` `--rules-file` | `3` `3` `5000` `40` `20` `null` |
| `crossover` | `mode` `rules` `sizes` `rulesRange` `candidates` `reps` `rulesFile` | `--mode` `--rules` `--sizes` `--rules-range` `--candidates` `--reps` `--rules-file` | `candidates` `500` `20,200,1000,5000,100000` `500,1000,2000,5000,20000` `1000` `3` `null` |
| `fair` | `rules` `candidates` | `--rules` `--candidates` | `300` `1000` |
| `scale` | `points` | `--points` | `30x100,100x200,300x1000` |
| `perf` | `iters` | `--iters` | `3` |
| `http` | `iters` | `--iters` | `10` |
| `load` | `endpoint` `concurrency` `duration` | `--endpoint` `--concurrency` `--duration` | `json` `25` `10000` |
| `memory` | `queryBatches` `replacements` `replacementsOnly` | `--query-batches` `--replacements` `--replacements-only` | `20` `10` `false` |
| `memoryScale` | `cells` `rules` `vertices` `candidates` `queryBatches` `replacements` | `--cells` `--rules` `--vertices` `--candidates` `--query-batches` `--replacements` | `1000x10,…,100000x100` `1000,10000,100000` `10,100,1000` `1000` `20` `20` |

`rulesFile: null` means synthetic mode; set it (or pass `--rules-file=…`) to run
against a real GeoJSON boundary file. Paths are repo-root-relative. There are no
environment variables.
