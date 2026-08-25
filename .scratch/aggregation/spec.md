# Aggregation: per-candidate analytics over the applicable rule set

Aggregation adds a per-candidate **analytics view over the applicable rule set** — the set resolution already materializes (ADR-0015) — computed natively on the rich path. It is the first fog item the shipped decision model unblocks, and it rides the existing query surface.

A query gains a top-level `aggregate` spec:

```json
{ "aggregate": { "count": true, "min": "speedLimit", "max": "speedLimit",
                 "sum": "speedLimit", "avg": "speedLimit", "coverage": true } }
```

Per candidate, over the **applicable rules** (spatial predicate holds + `where` admits + not excluded — DE-9IM, `withinDistance`, and `$activeAt` alike):

- `count` = the applicable-set size.
- `min`/`max`/`sum`/`avg` = numeric (Int/Float) aggregation of the named rule property; a rule whose property is missing or non-numeric is **skipped**; the aggregate is **absent** when no applicable rule contributes a numeric value. Each function names its own field (Mongo `$min: "$field"` idiom).
- `coverage` = **union coverage**: `geodesic_area(candidate ∩ union(applicable rules)) / geodesic_area(candidate)`, via `geo::BooleanOps::union` + `GeodesicArea` (both already in the engine). Point/MultiPoint candidates have zero area → coverage `0`.

The aggregate is carried as a per-candidate `aggregate` object in the rich JSON — both `query(...).toOutcomesJson()` matched outcomes and `resolve(...).toJson()` resolved outcomes — **absent** (not `null`) for notMatched/invalid candidates. It is rich-path only and lazy (like `includeOverlap`); the mask, `count()`/`summary()`, and the resolution winner/values are untouched, and aggregation never changes admission.

Validation is strict: unknown aggregate keys, a non-boolean `count`/`coverage`, a non-string numeric field, or an empty `aggregate` object → `SR_INVALID_QUERY` (the no-silent-misreading ethos).