# Quantitative overlap: overlapArea + overlapRatio (rich path)

Type: task
Status: resolved
Blocked by: 02

## Answer

Implemented in `cf4b9fa` (`feat(core): quantitative overlap area/ratio on the rich path`): `includeOverlap` returns `overlapArea`/`overlapRatio` on the rich query path per ADR-0012 (blocker 02 resolved).

## Question

Add quantitative overlap per ADR-0012, rich path only:

- `CandidateOutcome::Matched` gains an optional per-rule payload `{ overlap_area: f64, overlap_ratio: f64 }` (core). Computed from a `geo::BooleanOps` intersection (candidate ∩ rule) measured with `GeodesicArea` on numerator and denominator.
- `overlap_ratio = geodesic_area(candidate ∩ rule) / geodesic_area(candidate)` — dimensionless [0,1].
- `overlap_area` in m² (geo's native geodesic unit). No planar lon/lat treatment (Initial-plan §14).
- Hot-path `Uint8Array` mask unchanged; the `includeOverlap: true` flag is opt-in.
- napi (`node/src/lib.rs`): `queryRich` honors `includeOverlap` and serializes per-matched-rule `overlapArea`/`overlapRatio`; `query()` is untouched.

Tests: known-area fixtures with documented geodesic expectations (hand-computable lon/lat square overlaps); ratio bounds [0,1]; identical masks with and without the flag; `queryRich` shape with/without `includeOverlap`; node smoke.

Run: `cargo test --workspace` / `cargo clippy --workspace --all-targets`, node smoke under Node and Bun — green before commit.
