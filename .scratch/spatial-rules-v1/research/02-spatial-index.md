# 02 — Spatial index: primary-source research (2026-08-13)

Supports the Spatial index ticket. Sources: crates.io, docs.rs, GitHub README/CHANGELOG.

## `rstar` (pure-Rust R*-tree)

- v0.13.0 (~3 months old), MIT OR Apache-2.0, 38.5M downloads, MSRV 1.85, georust. https://crates.io/crates/rstar
- `RTree::bulk_load(Vec<T>)` — OMT top-down bulk-load, "preferred way… both runs faster and yields an r-tree with better internal structure." No separate `BulkLoader` in 0.13. https://docs.rs/rstar/latest/rstar/struct.RTree.html#method.bulk_load
- `locate_in_envelope_intersecting(envelope)` + allocation-free `_int` variants. https://docs.rs/rstar
- Crate benchmark: bulk-load 2000 items = 229.82 µs vs 1.4477 ms sequential. https://crates.io/crates/rstar
- geo's `PreparedGeometry` is R*-tree-backed; geo types plug into rstar via `GeomWithData` (`RTreeObject`). https://docs.rs/geo

## `geo` built-in structures (0.33.1)

- `geo::algorithm::rtree::RTree` **no longer exists** — the Spatial Indexing section points to `rstar::RTree::bulk_load` directly. https://docs.rs/geo
- `PreparedGeometry`, `IntervalTreeMultiPolygon`, `MonotoneChainMultiPolygon` are per-geometry accelerators, not multi-rule bbox indexes. https://docs.rs/geo
- Verdict: for ~30-rule bbox pre-filtering the geo-idiomatic path is `rstar::RTree::bulk_load` of `GeomWithData<Geometry, RuleId>`.

## `static_aabb2d_index` (flatbush port)

- v2.1.0 (2026-08-08), MIT OR Apache-2.0, 118k downloads, MSRV 1.86. https://crates.io/crates/static_aabb2d_index
- Static AABB index; build-once/query-many; `#![forbid(unsafe_code)]` by default. https://docs.rs/static_aabb2d_index
- API: `StaticAABB2DIndexBuilder::new(n)` → `add(…)` → `build()`; `query`, `visit_query`, `query_iter`. Per-query only.
- Benchmarks are internal before/after numbers; **no claim vs an R-tree**; smallest published item counts are 100/1,000.

## Other crates

- `spade` (Delaunay/Voronoi) and `kdtree` (point NN) — unsuitable for bbox filtering.
- No separate packed STRtree/BVH crate verified; `rstar::bulk_load` (OMT) is the closest maintained packed tree.

## Small static datasets (~30 items)

No crate publishes a 30-item crossover. rstar notes trees help "if many queries and only few insertions," but creating one is O(n log n) and performance "usually degrades to O(n)" for heavily-overlapping boxes. At n=30 a flat `Vec<Envelope>` scan — 30 inlined AABB tests, cache-resident, no allocation — is very likely ≥ a tree. Conclusion: measure, don't assume.

## Batch query

- `rstar`: per-query iterators, plus `intersection_candidates_with_other_tree(&other)` — a true tree-vs-tree candidate join (batch API if candidates are also indexed).
- `static_aabb2d_index`: per-query only.

## Options

1. **`Vec<Envelope + rule_id>` linear scan** — zero deps, 30 fast inlined tests/query, trivially correct; cons: O(30)/candidate with no pruning if rules grow.
2. **`rstar::RTree::bulk_load(GeomWithData<Geometry, id>)`** — packed R*-tree, ecosystem-standard, geo types plug in, batch join available; cons: per-query tree overhead likely slower than scan at n=30.
3. **`static_aabb2d_index`** — purpose-built static AABB index; cons: raw coords only, per-query only, lowest adoption.

**Recommended default:** linear scan (option 1) for 30 rules; precompute envelopes once at rule-load. Overturn only if a criterion benchmark (real 30 bboxes × 1,000 candidate envelopes, matching p50/p99, with a rule-count sweep 30→100→1,000) shows a tree beating the scan by a meaningful margin (>~10–20%).
