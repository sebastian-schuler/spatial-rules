# Deferred: envelopes by-id copy and single-representation ingestion

Type: task
Status: wontfix

## Why this is separate

Both were listed under ticket 04 but are not trims — each needs a design change
with real risk, so they are split out rather than bundled with the allocation
cleanups that landed.

## 1. `Ruleset.envelopes` is the by-id accessor, not a redundant copy

`Ruleset.envelopes: Vec<Rect<f64>>` (`core/src/runtime/ruleset.rs:37`) looks
like a duplicate of the R-tree's stored envelopes, but the index entries are
`(Rect, RuleId)` keyed by *envelope* — there is no `RuleId → Rect` lookup. So the
`Vec` is what gives `Ruleset::envelope(rule_id)` and the benchmark `RuleSource`
their O(1) by-id access.

Options to explore (pick during triage):

- Add an `envelope(RuleId) -> Rect` accessor to `SpatialIndex` and drop the
  `Vec`; `LinearScanIndex` can scan its entries, `RStarIndex` cannot without
  extra by-id storage (which would recreate the copy).
- Gate `envelopes` (and `Ruleset::envelope`) behind
  `#[cfg(any(test, feature = "benchmark"))]` if no production consumer uses it —
  it is currently only read by tests and the ladder. This is an API change; check
  the binding crates and the api-surface test first.
- Leave it: 32 B/rule = ~3.2 MB at 100k rules (~4% of the post-03 footprint).

## 2. Ingestion holds two coordinate representations

`core/src/runtime/ingestion.rs` parses into `geojson::GeoJson` (a DOM), then
copies each coordinate into `geo::Geometry`; the DOM lives until the features
are dropped. For the integration server — which loads `rules.geojson` directly —
the parse peak is roughly 2× the coordinate memory.

`geo` already enables its `serde` feature, so a single-representation ingest is
possible, but the `geojson` crate currently provides the feature handling that
would have to be reimplemented or preserved:

- `id` present as a JSON string **or** number (`extract_feature_id`,
  `id_to_string`), falling back to `properties.id`.
- the top-level `priority` foreign member (`extract_feature_priority`) and its
  typedness gate.
- permissive geometry with validity as a separate gate (ADR-0005).

Any rewrite must keep `core/tests/edge_matrix.rs`, `core/tests/ingestion.rs`,
and the error-matrix cases green (BOM, missing id, NaN/Infinity, antimeridian,
unsupported geometry types, skipped property types).

## Acceptance

- A measured drop in the ingestion/load peak on the integration-server path
  (the memory grid does not exercise it — add a load-shaped measurement).
- All ingestion/edge/error tests byte-identical.
- Any public API change recorded (CHANGELOG / ADR).

## Comments

**2026-09-17 (agent): item 1 decided (keep); item 2 still open.**

**1 — `Ruleset.envelopes` → keep.** The `Vec<Rect<f64>>` looks like a duplicate
of what the index stores, but it is the **by-id accessor**: the indexes are keyed
by envelope, not by `RuleId`, so `Ruleset::envelope(rule_id) -> Option<&Rect<f64>>`
returns a reference *into* this vector. Dropping it would force the accessor to
return by value (a breaking signature change) to save ~32 B/rule (~4% at 100k),
and it is now also the base box the lazy `withinDistance` fringe index is built
from (`Ruleset::query_fringe_into`). Not worth an API break.

**2 — single-representation ingestion → wontfix**, on two findings.

**(a) The premise is false.** The ticket (and the exploration behind it) claimed
"`geo` already enables its `serde` feature, so deserializing straight into geo
types would remove the redundant copy". Verified with a throwaway probe:
`geo::Geometry` deserializes from its *own* derived shape, not GeoJSON —

```
GeoJSON object -> geo::Geometry: unknown variant `type`, expected one of `Point`, ...
derived enum (x,y)      -> geo::Geometry: invalid length 1, expected struct Polygon with 2 elements
geo serializes Point    as {"Point":{"x":1.0,"y":2.0}}
geo serializes Polygon  as {"Polygon":{"exterior":[{"x":..,"y":..},...],"interiors":[]}}
```

So there is no "deserialize straight into geo types": it would mean hand-writing
a GeoJSON→`geo` coordinate deserializer, duplicating the `geojson` crate.

**(b) The achievable variant's win is negligible at the production shape.**
Keeping the `geojson` crate but **streaming one feature at a time** (needs a
hand-written, member-order-robust `Deserialize` for the top-level document, and
must reproduce every error message the edge matrix pins) would remove only the
*transient* whole-document DOM. Measured on 100,000 candidates
(28.9 MiB of GeoJSON, a temporary probe against `rss::snapshot`):

| | bytes |
|---|---|
| peak delta during `candidates_from_geojson` | 46.3 MiB |
| of which retained (converted candidates + slack) | 24.5 MiB |
| **transient whole-document DOM** | **~22 MiB** |

The documented production workload is **1,000 candidates per request** — its
input is ~0.29 MiB, so its DOM is ~0.2 MiB: nothing. The ~22 MiB only appears at
the 100k-candidate crossover/scale experiment, which is not a deployment shape.

**Net:** a risky rewrite of a load-bearing parser for a transient saving that is
invisible at the production shape. Revisit only if a container-budget
measurement shows the ingestion peak is actually binding, and then with the
streaming design above (not the `geo`-serde one).

If any of these is revisited, 1 needs a breaking signature change and 2 needs the
streaming deserializer plus a load-shaped measurement.
