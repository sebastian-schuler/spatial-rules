# Aggregation: per-candidate analytics over the applicable rule set

The shipped decision model (ADR-0015) materializes, per candidate, the ordered **applicable rule set** — the rules admitted by the spatial predicate, the `where` clause, and exclusions. Aggregation adds an **analytics view over that same set**, computed natively on the rich path, so an application can answer "how many zones apply here, what is the speed-limit range, and what fraction of this parcel do they cover" without re-running the evaluation or shipping rule properties to the client. It is requested as a query-level **`aggregate`** spec (mirroring the `includeOverlap` flag pattern) and carried as a per-candidate object in the rich JSON — `toOutcomesJson()` and `resolve().toJson()` alike.

The functions: **`count`** (the applicable-set size); **`min`/`max`/`sum`/`avg`** over a named rule property, each naming its own field (Mongo `$min: "$field"` idiom), restricted to numeric (Int/Float) values — a rule whose named property is missing or non-numeric is skipped, and the aggregate is **absent** (never `0`) when no applicable rule contributes; and **`coverage`** = union coverage — `geodesic_area(candidate ∩ union(applicable rules)) / geodesic_area(candidate)`, computed with `geo::BooleanOps::union` + `GeodesicArea` (the same spherical machinery `overlap_metric` uses), so the fraction is honest even when rules overlap each other; point/multipoint candidates have zero area → `0`. Aggregates work uniformly across DE-9IM, `withinDistance`, and `$activeAt` predicates because they all feed the same applicable set.

Semantics to keep in mind: aggregation is a **separate merge** over the applicable set — it is not the first-provider-wins `values` (that is resolution's job), and it is **rich-path only and lazy**, so the mask, `count()`/`summary()`, and the resolution winner/values are untouched. Validation is strict — unknown keys, wrong types, or an empty `aggregate` → `SR_INVALID_QUERY`. The aggregate object is absent (not `null`) for notMatched/invalid candidates.

## Considered Options

- **Per-rule aggregates across the batch (histograms)** — rejected for v1: a per-rule table is a different result shape (not aligned to candidate order); it is documented additive.
- **Wrapper-computed aggregation** — rejected: the rich JSON carries rule ids, not rule properties, so the wrapper cannot compute min/max/coverage without shipping the whole ruleset to JS.
- **A separate `aggregate()` API** — rejected: a query-level spec on the existing rich path (the `includeOverlap` pattern) needs no new API surface.
- **Max per-rule overlap ratio as coverage** — rejected: it double-counts when several rules cover the same area; the union is the honest "fraction covered" (per-rule overlap remains available via `includeOverlap`).

## Where it is computed (2026-09-01)

The aggregate is computed in the **core engine** (`evaluate`/`evaluate_resolve`)
and carried on the matched/resolved outcome as `aggregate: Option<Aggregate>`;
the bindings only serialize it and never re-derive the applicable rule ids. So
"computed natively on the rich path" is literal — it is not a wrapper or
binding-layer computation. `AggregateSpec::compute` remains the pure
implementation, called by the engine internally and by the benchmark suite.