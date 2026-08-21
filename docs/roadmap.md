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

## Gate: the precedence model

**No P1 implementation starts before the precedence/conflict-resolution model
is decided.** When rules overlap (country → city → school zone all matching one
point), the engine must answer *"which rule wins?"* — via priority values,
specificity, allow/deny, first-match, merge, or some combination. This is the
hardest design problem on the roadmap and every P1 feature (and the Postgres
value proposition) presupposes an answer. It should be settled as an ADR
before code.

## P0 — Memory benchmark (shipped)

Produced the facts later decisions cite. Harness + results in
`docs/benchmarks.md` §Memory (memory-benchmark ticket 01, resolved): memory
tracks **rule count, not coordinate count** (~1.2–2.7 kB/rule steady; 100k
rules ≈ 118–260 MiB), no per-replacement leak, ~65 MB peak in the production
container against a 128 MB bound.

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

## P2 — Realistic rules

4. **Temporal conditions** — day-of-week/time-window filters on rules
   (parking, congestion zones, delivery windows). Ship as a property-filter
   predicate first; time as a first-class indexed dimension stays in fog until
   demand proves out.
5. **Distance predicates** — `withinDistance`, `nearest`, proximity bands.
   Fits the R-tree naturally; unlocks geofencing positioning.

CRS/geodesic semantics must be **decided and documented before distance
lands** (planar vs geodesic, antimeridian, wrapping) even if only planar is
implemented — otherwise every distance result becomes ambiguous retroactively.

## P3 — PostgreSQL loader

Load rules directly from PostGIS without GeoJSON serialization
(`SpatialRules.fromPostgres({connectionString, table, ...})`). Easy win;
independent of the precedence decision. Positioning: **PostGIS owns the data
and large-scale spatial querying; this engine owns high-speed rule
evaluation.**

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
- **Aggregation** — count/min/max/coverage over matched rules; analytics use
  cases.
- **Route-aware queries** — zone segments along a LineString with fractional
  start/end; a higher-level module for routing engines.
- **CRS/geodesic correctness beyond the documented semantics** — geodesic
  distance models, polar regions.
- **WASM build** — same engine in Node/Bun/Deno/browser/edge/Python.
  Distribution concern, not a design one; the Rust core is ready whenever.
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
