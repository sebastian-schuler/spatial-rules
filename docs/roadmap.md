# Roadmap

Compiled 2026-08-21 from three brainstorming documents (feature expansion,
memory benchmarking, PostgreSQL involvement), distilled with priority tiers.
This document supersedes the originals wherever they disagree; every idea from
them survives here at least as a tiered or fog entry.

## Direction

The interesting opportunity is **not** adding more spatial predicates. The core
abstraction — *given many geometries, evaluate them against an indexed,
attribute-bearing ruleset extremely quickly* — is already strong (ADR-0002,
ADR-0007, ADR-0008). The direction is to evolve from a **matcher** into a
**spatial policy engine**:

> A high-performance engine for evaluating what is allowed, required, or
> applicable at a location.

Target layering:

```text
        Spatial Policy DSL          (fog)
      Decision Engine               (P1)
        priority / derived values / explanations
      Spatial Rule Engine           (P2)
        temporal filter / distance
      Spatial Core                  (shipped)
        Rust + R-tree + DE-9IM + typed properties
```

## Gate: the precedence model — settled (ADR-0015)

**Settled 2026-08-23 by ADR-0015: the resolution model.** When rules overlap
(country → city → school zone all matching one point), the engine answers
*"which rule wins?"* with numeric precedence: each rule carries a top-level
integer `priority` (higher wins; missing = 0; ties break by declaration order),
and resolution returns the **ordered applicable set**, its **winner** (head of
the order), and **derived values** (first-provider-wins merge of rule properties
down the order). Specificity and allow/deny were weighed and rejected as
built-ins (expressible later as data on top of priority); a composable chain is
the documented extension path. This was the hardest design problem on the
roadmap and every P1 feature presupposes it — it is now the ADR that P1
implementation cites.

## P0 — Memory benchmark (shipped)

Produced the facts later decisions cite. Harness + results in
`docs/benchmarks.md` §Memory (memory-benchmark tickets 01–03, resolved):
rulesets track **rule count, not coordinate count** (~1.2–2.7 kB/rule steady;
100k rules ≈ 118–260 MiB of ruleset; serving adds a per-thread prepared-geometry
memo — the geo 0.34 deferral), the ruleset is ~2–5× smaller than a turf.js
baseline holding the same data, no per-replacement leak, ~67 MB peak for the
30-rule production workload against a 128 MB bound. Tickets 02–03 made serving
memory **lazy and workload-proportional** (per-rule prepare on first touch) and
**re-verified the whole picture on Linux** (the deploy platform, in the pinned
container): the 100k×100 serving footprint dropped from ~1.8 GiB to ~282 MiB at
1,000 candidates, the cold-batch prepare spike (~1.9 s) collapsed to ~7 ms, warm
throughput is unchanged, and 50-swap probes prove the big cells oscillate in a
bounded sawtooth (glibc trim cycles) rather than leaking. The serving footprint
now beats turf at every cell except the trivial 10-vertex corner.

## P1 — From matches to decisions

The fork in the road. Three features that land together:

1. **Rule priority / resolution** — behind the gate above. Turns "which rules
   match?" into "what is the effective rule at this location?"
2. **Derived values** — rules produce values (`speedLimit = 30`,
   `taxRate = 0.21`), not just booleans: `resolve(point) → {field: value}`.
   Falls naturally out of resolution; turns the engine into a spatial lookup
   table whose keys are geometry.
3. **Explainability** — why did a rule fire: predicate, spatial vs property
   match, conditions evaluated. Resolution *is* the explanation; build them
   together.

The resolution model is decided (ADR-0015); the working spec and ticket plan
live in `.scratch/p1-resolution/` (spec + issues 01–05).

## P2 — Realistic rules (shipped)

4. **Temporal conditions** — day-of-week/time-window filters on rules
   (parking, congestion zones, delivery windows). Shipped as a property-filter
   predicate: the query carries an ISO-8601 `at` and a whole-clause `$activeAt`
   admits rules whose window properties (day bitmask + hour range) contain it
   (ADR-0017, ticket 02). Time as a first-class indexed dimension stays in fog
   until demand proves out.
5. **Distance predicates** — `withinDistance`, `nearest`, proximity bands.
   `withinDistance` shipped as a metric predicate — spherical great-circle
   (Haversine) meters, minimum distance with 0 if inside, a conservative
   bounding-circle pre-filter over the R-tree, and resolution admission parity
   (ADR-0016, ticket 03). `nearest` remains a documented Non-Goal (Initial-plan
   §72); proximity bands compose as repeated `withinDistance`.

The CRS/geodesic gate was settled by the 2026-08-23 grilling session into
ADR-0016: spherical great-circle (Haversine) meters, antimeridian-safe for
conformant (in-range) coordinates; the ellipsoidal Karney geodesic is the
documented higher-accuracy additive alternative.

## Aggregation (shipped)

Per-candidate analytics over the applicable rule set — `count`, `min`/`max`/
`sum`/`avg` over a named rule property, and `coverage` (geodesic union
fraction) — requested as a query-level `aggregate` spec and carried on the
rich path (ADR-0018, `.scratch/aggregation/`). Removed from fog.

## Distribution — WASM + Python (shipped 2026-08-25)

The engine now distributes beyond the Node napi addon (ADR-0019,
`.scratch/wasm/`):

- **Wasm** — npm `spatial-rules-wasm` (`wasm/`): a `wasm-bindgen` build
  (`wasm-pack --release --target bundler`) exposing the Ruleset-level subset —
  `build`/`query`/`resolve` (mask as `Uint8Array`), the rich JSON views, and
  `toCanonical`. No `replace`/`stats` (their clock observability is degenerate
  on wasm — no clock) and no async. Consumable by browser bundlers, Node ESM,
  and Deno; release blob 829 KB (≤ 2 MB budget). Smoke runs under node and
  deno.
- **Python** — PyPI `spatial-rules` (`python/`): PyO3 + maturin, abi3
  `cp39-abi3` wheels covering CPython 3.9–3.13. The **full** Engine surface —
  `query`/`resolve`/`query_rich`/`resolve_rich`/`replace`/`to_canonical`/
  `stats` — with Pythonic dict/list in/out. JSON serialization is identical to
  the napi/wasm paths (shared `spatial-rules-bindings-common`). pytest smoke on
  CPython 3.11 + 3.13.
- **CI/release** — `wasm` and `python` CI jobs build + smoke both packages;
  release-please tags/releases both from the same Conventional-Commits feed.

## P3 — PostgreSQL loader (deferred 2026-08-24)

Load rules directly from PostGIS without GeoJSON serialization
(`SpatialRules.fromPostgres({connectionString, table, ...})`). **Deferred, not
rejected**: the loader is pure ingestion convenience — it adds no engine
capability. The "without GeoJSON serialization" mechanism only holds on a
native-Rust driver path (async in the napi binding, TLS, pooling, EWKB
parsing) that is not the "easy win" the roadmap assumed; the JS-driver
alternative (`pg` + `ST_AsGeoJSON`) re-serializes through GeoJSON and adds a
runtime dependency. No concrete PostGIS-backed deployment is driving demand.
The Postgres **phases 2–5** in fog (live sync, predicate pushdown, native
extension) remain the compelling direction once the decision semantics are
worth embedding. Positioning: **PostGIS owns the data and large-scale spatial
querying; this engine owns high-speed rule evaluation.**

## Fog — not yet specified

Suspected directions that hang on open questions; graduate when sharp:

- **Streaming geofencing** — `watch(candidate)` emitting ENTER/STAY/EXIT. A
  different product surface (stateful, per-candidate subscriptions); revisit
  after P1/P2 prove the decision model.
- **Rule composition / expression language** — logical spatial expressions
  (`IN x AND NOT IN y AND WITHIN 100m OF z`). A compiler over primitives that
  don't exist yet; re-visit after P1.
- **Declarative rule DSL** — YAML/DSL compiled to rulesets; versionable,
  diffable, auditable. Depends on composition and derived values existing.
- **Temporal indexing** — time as first-class indexable dimension (beyond the
  P2 filter).
- **Route-aware queries** — zone segments along a LineString with fractional
  start/end; a higher-level module for routing engines.
- **CRS/geodesic correctness beyond the documented semantics** — geodesic
  distance models, polar regions.
- **Compiled/mmap persisted format** — canonical JSON with recompile-on-load
  is decided (ADR-0013); a fully-compiled binary/mmap format was rejected for
  current scale. Revisit if ruleset sizes or boot-time demands grow by orders
  of magnitude.
- **H3/S2/geohash cells** as native primitives.
- **Postgres phases 2–5** — live sync (LISTEN/NOTIFY → CDC), hybrid execution
  with predicate pushdown, native Postgres extension, planner integration.
  All compelling only once the engine has decision semantics worth embedding;
  see postgres-involvement analysis. ST_Subdivide-based ingestion
  (fragment-per-rule-ID) is worth remembering for any large-ingestion path.

## Explicitly rejected (for now)

- Wrapping PostGIS queries from Node as a "Postgres integration" — PostGIS
  already does spatial filtering extremely well; the value is everything
  *after* the filter.
- Adding 30 more topological predicates — diminishing returns versus the
  decision layer.
