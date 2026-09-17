# withinDistance: recover pre-filter selectivity (spherical index over-approximates)

Type: task
Status: resolved

## Why

Ticket 09 fixed a correctness bug by indexing each rule's **great-circle**
envelope instead of its planar box (`core/src/model/spherical_envelope.rs`).
That box is a single rectangle around a rule's arc-sampled extent, so a rule
with long **diagonal** edges grows a lot — a 40° edge at 50°N bulges ~1.7°
(~190 km) poleward of its planar box. More rules then reach the expensive
`HaversineClosestPoint` confirm.

Same-session A/B (`bun run bench samples --filter=batch_within_distance`,
planar index temporarily restored):

| bench | planar (buggy) | spherical (correct) | delta |
|---|---|---|---|
| `within_distance_mask` | 131.9 ms | 336.9 ms | **+156%** |
| `within_distance_full` | 131.6 ms | 336.7 ms | **+156%** |
| `mask_baseline` (points, intersects) | 1.50 ms | 2.74 ms | +83% |

`withinDistance` is now ~50× the intersect mask instead of ~21×. Correctness is
not negotiable; the selectivity is.

## Directions

1. **Per-edge fringe entries (preferred).** Index the tight planar box as today
   and add an extra R-tree entry *only* for the arc region that escapes it (a
   box per bulging edge, skipped when the bulge is below the ~11 m margin).
   Small rules (the memory-grid shape) add no entries; country-scale rules add a
   few. Requires the index to accept multiple entries per rule id and the query
   path to dedup — `RStarIndex::query_envelope_into` already sorts + dedups;
   `LinearScanIndex` needs checking.
2. **Sub-boxes per rule** (split along latitude): simpler than per-edge, but
   grows memory with a fixed multiplier.
3. **Revisit the model.** Treat rule edges as planar (lon/lat straight lines,
   the GeoJSON/PostGIS-`geometry` convention) so the planar index is exact.
   Rejected unless the ADR reopens: ADR-0016 chose spherical great-circle
   distance, and the confirm/oracle are built on it.

## Acceptance

- The ticket-09 counterexample still returns a match (no conservativeness lost):
  `core/tests/distance.rs::high_latitude_arc_bulge_does_not_drop_a_within_rule`
  stays green, and the un-ignored proptest stays green across seeds.
- `batch_within_distance` recovers most of the 132 ms baseline.
- Any index-shape change reflected in the api-surface tests and ADR-0016.

## Comments

**2026-09-17 (agent): resolved — two indexes, one lazy fringe box per rule.**

The key realisation: `withinDistance` is **spherical** (the confirm walks
great-circle arcs) while the DE-9IM predicates are **planar** (`geo::relate`),
where the planar box is already exact. So a rule now has **two** index sets:

- the existing **planar** index (unchanged) — every DE-9IM predicate;
- a **fringe** index — one great-circle box per rule, only for rules whose arcs
  escape their planar box (`core/src/model/spherical_envelope.rs::fringe_box`),
  consulted *only* by the distance pre-filter, which unions the two hit sets
  (`PreparedQuery::extend_distance_hits`, with a second reusable
  `fringe_scratch` buffer so there is no per-candidate allocation). Built
  **lazily** on the first distance query, so a ruleset that never runs one pays
  nothing.

`RuleAccess` gained `query_fringe_into`; the public `Ruleset::envelope` accessor
and the benchmark `RuleSource` stay planar.

**First attempt (rejected) — edge-granular fringe boxes.** The initial version
emitted one box *per escaping edge*, which recovered far more selectivity
(`within_distance_mask` 337 → **185 ms**) — but it grows with **vertices**, not
rules. At low and mid latitude even a short edge's arc leaves the exact planar
box, so nearly every boundary edge added an entry (~1M entries at 100k rules).
The memory grid caught it: **816 → 1626 B/rule, 77.8 → 155.1 MiB** — a 2×
regression in a memory workstream, so it was reverted to one box per rule.

**Final measured state** (`bun run bench samples`; memory via
`memory-scale --rules=100000 --vertices=10`):

| bench | planar-only (buggy) | arc-hull, eager, one index | **shipped: split + lazy fringe** |
|---|---|---|---|
| `within_distance_mask` | 131.9 ms | 336.9 ms | **~392 ms** |
| point `mask_baseline` (DE-9IM) | 1.50 ms | 2.74 ms | **1.55 ms** |
| resolve / temporal / aggregation baselines | — | +25% vs planar | **back to planar** |
| 100k×10 `bytes/rule` | 816 B | 816 B | **818 B** |

So the DE-9IM collateral is fully removed and the memory cost is zero unless a
ruleset actually runs `withinDistance`. The residual is pre-filter selectivity on
the distance surface — the price of a box filter over arc geometry; the extra
confirms are rules genuinely near an arc that the planar box missed (cases the
old index got wrong). A bounded middle ground (K clustered boxes per rule) is the
obvious next step if distance throughput demands it, but it trades memory again.

**Acceptance:** the ticket-09 counterexample still matches, the un-ignored
proptest stays green across seeds, and the suite + clippy are green. The
`batch_within_distance` "recover most of the 132 ms" criterion is **not** met —
it sits at ~392 ms — and is reported as such rather than claimed.

**Bug found and fixed during this ticket:** the first implementation never
cleared the accumulator between candidates (the index query clears only its own
temporary), so it grew unbounded and the sort/dedup went quadratic — 1.98 s per
batch. Now `admit_within_distance` clears the accumulator once, before the
(up to three) envelope queries.

Not done (deliberate): the `batch_rich_json` cells read ~6% slower than in the
earlier run, which is unexplained (that path does not use the index) and is 2 µs
on a 35 µs bench; left as a note rather than chased.
