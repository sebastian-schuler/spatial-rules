# Spatial predicates: covers, covered_by, touches, overlaps

Type: task
Status: ready-for-agent

## Question

Extend `SpatialPredicate` (`core/src/query.rs`), its `FromStr`, and `spatial_predicate_holds` (`core/src/ruleset.rs:64`) per ADR-0012, in the existing candidate-relates-to-rule direction:

- `covers` → `matrix.is_covers()`
- `touches` → `matrix.is_touches()`
- `overlaps` → `matrix.is_overlaps()`
- `covered_by` → custom 4-pattern DE-9IM match (`T*F**F*** | *TF**F*** | **FT*F*** | **F*TF***`) on the candidate→rule matrix (geo has no `is_covered_by`).

Distance predicates are **out of scope** (metric/CRS feature; nearest-neighbor is a §3 non-goal).

Cross-check: add `@turf/boolean-covers` + `@turf/boolean-touches` to `benchmarks/js` (`boolean-overlap` already pinned); extend the turf suite and the ADR-0008 semantics matrix with the four predicates, covering boundary-touch cases (e.g., `covers` true where `contains` false on a shared boundary; `overlaps` only for same-dimension interior overlap).

Tests: directional semantics per predicate (mirroring the `contains`/`within` directional tests in core/tests/query.rs); boundary-touch cases; turf cross-check green; node smoke passes the new predicate strings.

Run: `cargo test --workspace` / `cargo clippy --workspace --all-targets`, and `npm run cross-check` in `benchmarks/js` — green before commit.
