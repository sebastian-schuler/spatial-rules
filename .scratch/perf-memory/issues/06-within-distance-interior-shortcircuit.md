# withinDistance: short-circuit interior points

Type: task
Status: wontfix

## Goal

Skip the trig-heavy closest-point search when the candidate is already inside
the rule (distance is 0 by definition).

## Problem

`min_haversine_distance` (`core/src/runtime/evaluate.rs:117`) always calls
`rule.haversine_closest_point`. For a point already inside the rule the answer
is 0 and the search is wasted. `withinDistance` is the ~21× geofencing surface
(`docs/benchmarks.md:149`), and interior hits are common for GPS-fix workloads.

## Plan

- Short-circuit with a containment/intersects test on the candidate point
  before `haversine_closest_point`; return 0 on a hit.
- Keep the conservative bounding-circle pre-filter over the R-tree unchanged.

## Acceptance

- `batch_within_distance/*` interior-heavy cells improve; results byte-identical.
- Measure with `bun run bench samples --filter=batch_within_distance`
  (perf-memory 01) — judge the delta against the run-to-run spread.

## Comments

**2026-09-16 (agent): wontfix — the short-circuit is already there.**

Checked the primary source before writing any code
(`georust/geo` main, `geo/src/algorithm/haversine_closest_point.rs`).
`HaversineClosestPoint for Polygon` already begins with:

```rust
if self.contains(from) {
    return Closest::Intersection(*from);
}
```

and the same guard opens the `Triangle` and `Rect` impls; `MultiPolygon`
delegates through `multi_geometry_nearest`, which reaches those per-member
checks. Rules are validated to Polygon/MultiPolygon, so **every** interior-point
case already returns before the segment scan.

So the ticket's premise — "runs the full closest-point search even for points
inside a rule" — is false for the geometry types we admit. Adding our own
`point.intersects(rule)` guard would duplicate the `contains` check on the
interior path and add a second containment test to the *non*-interior path,
which is the common one. That is a regression, not an optimisation.

The real `withinDistance` cost (`batch_within_distance` ≈ 138 ms vs a 1.6 ms
point-intersects mask) is the closest-point segment scan over country-scale
polygons for points that are genuinely *outside* — plus the per-rule `contains`
itself. Beating that needs a different idea (e.g. a cheaper containment
pre-filter or greater pre-filter selectivity), not this one; it is not in scope
here and would need its own evidence.

One residual micro-idea, explicitly rejected as not worth it: the `MultiPoint`
branch folds over every point even after one returns 0, so a `return 0.0` on the
first interior point would save work — but the benchmark candidate set is single
points (`dataset::point_candidates` are envelope centres), so it would not show
up, and it is unrelated to the ticket's claim.
- `withinDistance` invalid-candidate semantics (non-point candidate →
  `SR_INVALID_QUERY` outcome) unchanged.
