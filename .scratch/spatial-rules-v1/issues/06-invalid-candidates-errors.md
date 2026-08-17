# Invalid candidate handling and error model

Type: grilling
Status: resolved
Blocked by: 01

## Question

Decide invalid-geometry behavior and the error taxonomy (§42 item 11; §34–§35):

1. **Rules** — invalid rule geometry prevents ruleset publication; define what validation runs at compile.
2. **Candidates** — per-candidate `matched / not_matched / invalid` rather than whole-batch failure (§34); define exactly which malformed inputs mark a candidate invalid vs reject the query.
3. **Error taxonomy** — distinguish invalid GeoJSON, invalid geometry, invalid query, invalid property predicate, ruleset construction failure, unsupported geometry type / predicate / operator, native/runtime error (§35); assign stable error codes.
4. **Node binding mapping** — which errors become which JavaScript `Error` objects/codes.

Locked decision becomes an ADR in `docs/adr/`.

## Answer

Locked (grilling 2026-08-17, recommendations accepted):

- **Invalid rules at compile:** strict — any invalid rule geometry fails the whole ruleset construction; a ruleset is atomic (§34).
- **Invalid candidates:** per-candidate — the batch completes, that candidate gets mask `2` / an `Invalid { reason }` outcome; only malformed top-level input (bad JSON, not a FeatureCollection) is a query error, not per-feature.
- **Error taxonomy:** `SpatialError { code, message }` with stable `SR_*` codes covering the §35 categories (`SR_INVALID_GEOJSON`, `SR_INVALID_GEOMETRY`, `SR_INVALID_QUERY`, `SR_INVALID_PROPERTY_PREDICATE`, `SR_RULESET_CONSTRUCTION_FAILED`, `SR_UNSUPPORTED_GEOMETRY_TYPE`, `SR_UNSUPPORTED_SPATIAL_PREDICATE`, `SR_UNSUPPORTED_PROPERTY_OPERATOR`, `SR_NATIVE`); construction/query failures throw; per-candidate invalid stays in the result.
- **Node binding:** `SpatialRulesError extends Error` with `.code`; candidate invalid is result data, never thrown.

Asset: [ADR-0005](../../../docs/adr/0005-invalid-geometry-and-errors.md).
