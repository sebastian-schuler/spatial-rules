# Test comprehensiveness: audit + expand the suite

Type: task
Status: ready-for-agent

## Question

Make the test suite expansive and systematic. Today: ~68 core tests (hand-computed literals; `complex.rs` uses a seeded generator), node smoke + clean-install, integration smoke + memory, turf cross-checks. Gaps found in the 2026-08-19 audit: **no test CI workflow** (only `prebuild-publish.yml`), **no property-based testing** (`proptest`/`quickcheck` absent), **no fuzzing**, and no explicit error-model or edge-input matrix.

Targets (prioritized):

1. **Property-based tests (add `proptest` dev-dep to core):**
   - Ingestion: random valid/invalid/degenerate GeoJSON → correct `Candidate`/`Rule` or the right `SR_*` error, never a panic.
   - `WhereExpr::parse` + eval: random where clauses against random typed properties; invariant `eval` is total (no panic) and matches the documented missing/mismatch = non-match rule.
   - DE-9IM invariants on random geometry pairs: `intersects ⇔ ¬disjoint`; `contains → intersects`; `within ⇔ contains` on the reversed pair (ADR-0008).
   - Batch invariants: output aligned to input; `Invalid` outcome (not batch failure) for unsupported/invalid candidate geometry.

2. **Error-model matrix:** every `ErrorCode` / `SR_*` string is reachable by a documented input; the same code+message surfaces across Node and Bun (`SpatialRulesError.code`).

3. **Edge/input matrix:** empty collections; features without ids; UTF-8/BOM; malformed JSON; wrong property types; NaN/Infinity coords; antimeridian/pole coords; very large/small values; degenerate (zero-area, self-touching) geometries; unsupported geometry types → `Invalid` outcome, never a panic.

4. **Fuzz (cargo-fuzz, follow-up sub-task):** `candidates_from_geojson`, `rules_from_geojson`, `Query::from_json` / `WhereExpr::parse` — no panic, no unbounded allocation (DoS).

5. **Test CI + runtime matrix:** add `.github/workflows/test.yml` running `cargo test --workspace` + `cargo clippy --workspace --all-targets` + node smoke on the supported runtime matrix (Node 22/24/26 + Bun, per ticket 09) and OS matrix (windows/linux); non-host clean-install from packed tarballs.

6. **Regression gate:** a documented test matrix (in docs) mapping feature → test file, so new post-v1 features (tickets 01–06) can't silently drop coverage; each already has a Tests section — this ticket makes it enforceable.

## Notes

- May be split into sub-tickets (e.g. proptest+edge / test-CI+matrix / fuzz) during planning; the cargo-fuzz target may be a separate follow-up.
- Definition of done: `cargo test --workspace` + clippy green with the expanded suite; proptest runs a bounded case count under `cargo test` (default) and a larger count in CI; `test.yml` green on the matrix; test matrix documented; fuzz corpus no-panic (best effort within this ticket).

Run: `cargo test --workspace` / `cargo clippy --workspace --all-targets`, node + Bun smoke.
