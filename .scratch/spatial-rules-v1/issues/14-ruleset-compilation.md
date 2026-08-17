# Rust core: ruleset compilation

Type: task
Status: open
Blocked by: 13

## Question

Build immutable `Ruleset` compilation in `spatial-rules-core` (ADR-0001/0002/0003; use the tdd skill):

- Precomputed rule envelopes + a `SpatialIndex` trait whose default is `rstar::RTree::bulk_load` (ADR-0002).
- Property equality + `$in` indexes for every property at compile; range predicates scanned (ADR-0003).
- `Ruleset` exposing `RuleId` → geometry/properties; fully built before publication, immutable after.

Ruleset builds from a FeatureCollection; indexes queryable; unit tests green.
