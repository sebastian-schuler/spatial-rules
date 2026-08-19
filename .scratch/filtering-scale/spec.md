# Filtering at scale — refocused concept

Refocused 2026-08-19. The "geometry-operations / turf-replacement" widening
(ADR-0014, since deleted) and the naming/marketing thread are **parked**; the
concept returns to its original aim: **filtering large datasets of geospatial
data**.

## Concept

A high-performance, in-process engine that filters large collections of
candidate geospatial features against an indexed, attribute-bearing,
replaceable ruleset — keep/drop each feature based on spatial + property
predicates, from Node/Bun.

Scale target: **per-request batches** (larger than the current ~1,000 per
request), not offline dataset ETL. The engine stays request-shaped and
embedded; "filter bigger batches predictably" (scale, memory, latency) is the
improvement axis.

## In-scope use cases

- **Regulated-zone filtering** (VRA) — the original: drop imagery footprints
  that intersect an applicable zone unless the user is exempt.
- **Permission / library filtering** — "hide items that intersect a rule unless
  the user is exempt", expressed as a `where` over rule metadata +
  `excludeRuleIds`.

Everything else is an "advance into" candidate, not a commitment.

## Evidence (done 2026-08-19)

Crossover levels added to `benchmarks.json` + `docs/benchmarks.md` §5:

- **100k candidates** (500 rules, synthetic): addon 456 ms vs turf 1,342 ms
  (2.9×, 62,092 matched); near-linear scaling from 5k (22.4 ms).
- **20k rules** (1,000 candidates, synthetic): addon 5.6 ms vs turf 61.8 ms
  (11.0×); the addon stays flat 4.1 → 5.6 ms over 40× rules.
- One-time ruleset build grows with rules (~5.3 s at 20k); steady query stays
  ~flat. The build is fine for the weekly-replacement lifecycle.

## Roadmap tickets

- `issues/01-point-candidates.md` — Point/MultiPoint candidates (next engine
  investment; unlocks point-based filtering / geofencing-style checks).
- `issues/02-whole-clause-negation.md` — top-level `$nor` / whole-clause
  negation on `where` (exemption logic engine-side).
- `issues/03-filter-return-shapes.md` — filter return shapes
  (`filteredGeojson`, `filteredFeatures`, object `queryRich`, `keep`-indices).
- `issues/04-npm-publish.md` — registry publish of the prebuilt per-platform
  packages (the remaining operational step).

No blocking edges between tickets — each is independently actionable.

## App-side adoption (out of this tracker)

Overlap-ratio grading in the VRA flow — zero engine code; internal app work,
not this repo's tracker.

## Parked / out of scope

- Geometry-operations surface (union / difference / dissolve / buffer) — the
  ADR-0014 widening, reverted 2026-08-19.
- Validation primitives as an engine surface; provider-specific tasking rules
  stay application-side hooks if ever needed.
- Naming / marketing / "what is it" pitch.
