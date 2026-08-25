# CRS/geodesic and distance-semantics ADR (roadmap gate)

Type: decision
Status: resolved

## Question

Per the roadmap gate, CRS/geodesic semantics must be decided and documented before distance lands ("decided and documented before distance lands (planar vs geodesic, antimeridian, wrapping) even if only planar is implemented — otherwise every distance result becomes ambiguous retroactively"). Settled by the 2026-08-23 grilling session into ADR-0016: spherical great-circle (Haversine) meters, minimum-distance `withinDistance` admission, bounding-circle pre-filter, strict distance validation.

## Comments

> *Settled by the P2 grilling session (2026-08-23).*

## Agent Brief

**Decision:** Distance is measured with the spherical great-circle (Haversine) model in meters, consistent with the engine's spherical geodesic-area stance (Initial-plan §14). `withinDistance` is the v1 predicate: minimum candidate↔rule distance, 0 if inside, symmetric; evaluation is a conservative bounding-circle pre-filter over the R-tree plus exact `HaversineClosestPoint` + `Haversine.distance` confirm; the query shape `{spatial: {predicate: "withinDistance", distance: <meters>}}` is strictly validated. Ellipsoidal Karney geodesic and `nearest` are documented additive/deferred.

## Answer

Recorded in `docs/adr/0016-distance-predicates.md`. The ADR is the authoritative reference for ticket 03 (`withinDistance`).