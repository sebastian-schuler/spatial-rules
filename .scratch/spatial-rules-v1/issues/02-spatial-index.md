# Spatial index choice for small static rulesets

Type: grilling
Status: resolved
Blocked by: 01

## Question

Decide the spatial index for ~30 static rule geometries (§42 item 2; §15):

- Candidate structures: R-tree (e.g. rstar), packed STRtree / static bounding-box hierarchy, or plain bounding-box filtering.
- Optimize for: small static rule sets, frequent batch reads, infrequent rebuilds, ~1,000-candidate queries (§15).
- Decide whether the index queries per candidate or per batch, and how it feeds the pipeline `candidate → bbox filter → possible rules → property filter → exact predicate` (§17).
- Algorithm benchmarks A–F (§32) will validate the choice, so the ticket only needs the initial pick plus a definition of when the benchmark would overturn it.

Locked decision becomes an ADR in `docs/adr/`.

## Answer

Locked (grilling 2026-08-13), with the constraint that the rule count may grow from ~30 to hundreds+ (§4.1 — no hard limit):

- **Default index:** `rstar::RTree::bulk_load` of `GeomWithData<Geometry, RuleId>` — packed R*-tree, built once per ruleset, static. Chosen over a linear scan because a scan's O(rules)/candidate cost degrades as rules grow to hundreds, while the tree scales from 30 to 1,000+ rules on the same query path.
- **Envelopes** precomputed once at rule-load; candidates are not indexed.
- **Query granularity:** per-candidate envelope lookup (`locate_in_envelope_intersecting` / `_int`). Batch tree-to-tree join (`intersection_candidates_with_other_tree`) is a later optimization if profiling shows per-candidate overhead matters.
- **Abstraction:** a `SpatialIndex` trait in core so the benchmark ladder can swap scan (C) vs tree (D); the linear scan is kept as the benchmark baseline, not the shipped default.
- **Overturn criteria:** a criterion sweep (30 → 100 → 1,000 rules × ~1,000 candidate envelopes, matching p50/p99) decides the final shipped pick; the tree stays default unless the scan wins by >~10–20% at scale.

Assets: [research/02-spatial-index.md](../research/02-spatial-index.md) · [ADR-0002](../../../docs/adr/0002-spatial-index-rstar.md).
