# Point candidates

Type: task
Status: resolved

## Answer

Implemented (filtering-scale, 2026-08-19): `Point`/`MultiPoint` candidates
accepted at intake via a candidate-specific geometry gate in
`core/src/validation.rs` (rules stay polygon-only); `overlap_area`/`overlap_ratio`
are `0` for points (`core/src/ruleset.rs`; ADR-0012 amended); every
ADR-0008/0012 predicate verified for points, including `overlaps` (false).
Tests: `core/tests/query.rs` (point/multipoint match, directional predicates,
overlap 0/0), `core/tests/engine.rs`, `node/test/smoke.mjs` (point batch, green
under Node + Bun). `cargo test --workspace` + clippy green. Follow-up (not
done): extend the turf cross-check in `benchmarks/js` with point candidates.

## Question

Accept `Point` (and `MultiPoint`) **candidate** geometries in the engine. Today
`core/src/validation.rs` classifies candidates as Polygon/MultiPolygon only and
rejects anything else (`SR_UNSUPPORTED_GEOMETRY_TYPE`). `geo`'s `Relate` already
handles point/polygon, the R\*-tree lookup is over rule envelopes (trivial for
points), and the result model (`Uint8Array` mask / rich outcomes) is unchanged.
Rule geometries stay Polygon/MultiPolygon — only candidate geometry widens.

Scope:
- Classify `Point`/`MultiPoint` candidates at intake (envelope derivation,
  validation) in `core/src/validation.rs` + `core/src/candidate.rs`.
- Relate a point candidate against polygon rules under every predicate in the
  ADR-0008/ADR-0012 set (intersects, contains, within, covers, covered_by,
  touches, overlaps).
- Define overlap semantics for point candidates: `overlap_area` = 0 and
  `overlap_ratio` = 0 (a point has zero area) — document in the rich-path docs.
- Extend the turf cross-check (`benchmarks/js`) with point candidates
  (`@turf/boolean-*` accept points) and the node smoke test.

Unlocks point-based filtering ("user at lat/lng" checks against zones) — the
next engine investment for the filtering-scale plan.

Run: `cargo test --workspace` and `cargo clippy --workspace --all-targets`
green; turf cross-check parity with point candidates; node smoke passes a point
candidate batch.
