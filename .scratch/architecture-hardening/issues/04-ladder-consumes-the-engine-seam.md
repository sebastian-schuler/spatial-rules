# 04 — Ladder consumes the engine seam, one knob per rung

Type: task
Status: resolved
Blocked by: 03 (reuse the spatial-index result buffer)

Origin: 2026-08-19 architecture review, candidates 3 + 4.

## What to build

Make the algorithm ladder one coherent instrument: each rung differs from its neighbour by exactly one variable (spatial index on/off, prepared geometries on/off), every rung drives the engine through its public seams, and the documented speedups are attributed to the rungs that actually produced them. Today two rungs are hand-rolled relate loops with zero engine overhead, two call the full engine path, and one duplicates the engine's own per-candidate pipeline — so a "ladder" step changes several variables at once and one rung can silently drift from the engine it claims to represent.

**Prefactor first:** expose rule access by opaque id on the ruleset's public seam (a by-id accessor for geometry and prepared form), so the ladder stops rebuilding its own id-to-position map and the positional storage contract stays internal. Then delete the hand-rolled duplicate pipeline and drive each rung through the engine's seams (index kind, rule source, prepared query).

**Fix the attribution:** the docs currently credit prepared geometry with the dominant ~23–34× speedup, but the ladder numbers show the envelope/bbox filter is the dominant lever at these shapes (B→C ≈ 24×) with prepared geometry a small multiplier on top (E vs C ≈ 1.4×). The two levers are conflated in the docs; separate them so each speedup names the rung and the variable that produced it.

## Acceptance criteria

- [ ] A caller can fetch a rule's geometry and prepared form by opaque id through the ruleset's public seam; the ladder no longer rebuilds a positional id-to-index map
- [ ] Every ladder rung drives the engine through its public seams; the hand-rolled copy of the query pipeline is deleted
- [ ] Adjacent rungs differ by exactly one variable (index on/off, prepared on/off), and each rung's measured cost envelope is documented
- [ ] `docs/benchmarks.md` attributes each speedup to the rung + variable that produced it (envelope/bbox filter vs prepared geometry separated)
- [ ] Ladder results reproducible; build, prepare, and query groups all still report

## Answer

Implemented. `Ruleset::prepared()` returns a `PreparedRuleGeometries` handle
indexed by opaque `RuleId`, so the ladder no longer rebuilds a positional
id→index map. Every rung now drives the engine through its seams (rule source,
envelope query, prepared form) and differs from its neighbour by exactly one
variable (B→C bbox, C→D index, B→E prepared, D→F prepared+index). Measured
separation: prepared geometry ≈29× (B→E), bbox/index ≈1× at 30 large rules;
`docs/benchmarks.md` attribution corrected.
