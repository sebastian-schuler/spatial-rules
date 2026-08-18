# Persisted rulesets: canonical JSON format, recompile on load

Type: task
Status: ready-for-agent

## Question

Add canonical ruleset serialization per ADR-0013 (deploy-time precompile — validation at build time, deterministic canonical rules, near-identical load time):

- Add `serde = { version = "1", features = ["derive"] }` to the workspace; derives on `Rule`/`PropertyValue` (+ rule/property index value types as needed). `geo_types::Geometry` and `geojson` are already serde-serializable in the pinned versions.
- Core: `Ruleset::to_canonical()` / `Ruleset::from_canonical(&[u8])` — canonical **rules** JSON, not compiled indexes. `from_canonical` re-runs the full build (validation, envelopes, rstar index, property index) and assigns a **fresh `Ruleset.id`** (never persist the id — avoids `NEXT_RULESET_ID` restore and prepared-cache collisions per ADR-0010).
- napi: `SpatialRuleset.toJSON()` / `SpatialRuleset.fromCanonical(buf)`; load flows through the existing `Engine` atomic swap so a failed load keeps the old ruleset.
- Error model: reuse `SR_*` codes for invalid canonical input (malformed JSON, invalid geometry).

Tests: round-trip (to → from → identical rule ids/properties/geometry); fresh id on load; invalid canonical input errors and leaves the engine untouched; node smoke `toJSON`/`fromCanonical`; replace-after-load.

Run: `cargo test --workspace` / `cargo clippy --workspace --all-targets`, node smoke — green before commit.
