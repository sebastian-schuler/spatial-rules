# Build & load: remove duplicate and transient allocations

Type: task
Status: resolved

## Goal

Trim build-time peak and steady state; remove the double-representation on
ingest.

## Plan

- `Ruleset.envelopes` (`core/src/runtime/ruleset.rs:37`) duplicates envelopes
  the R-tree already holds and is only read by `Ruleset::envelope()` and the
  benchmark `RuleSource`. Drop it or derive it on demand; `index_entries`
  (`:142`) makes a third transient copy.
- `Ruleset.ids` (`ruleset.rs:122`) clones every rule id string into a
  `HashMap<String, RuleId>`; use `Box<str>` keys or an index-based map.
- `EqualityIndex::build` (`core/src/indexing/property_index.rs:38`) clones the
  key and value on every rule×property even when the entry exists; clone only
  on miss.
- `Ruleset::from_canonical` (`ruleset.rs:288`) parses to `serde_json::Value`,
  scans for priority, then re-parses `Vec<Rule>` — a second full pass over the
  geometry. Deserialize once.
- Ingestion (`core/src/runtime/ingestion.rs:14`) holds the GeoJSON DOM and the
  `geo::Geometry` at once; deserialize straight into `geo` types (its `serde`
  feature is already enabled) to halve the parse peak. This is the real
  integration-server load peak.

## Acceptance

- Build peak and steady state improve in the memory-scale grid; canonical load
  no longer double-parses.
- Canonical round-trip, ingestion edge-matrix, and ruleset build/validation
  tests green.
- `docs/benchmarks.md` updated with the re-recorded build/steady numbers.

## Comments

**2026-09-16 (agent): resolved in part; two sub-items deliberately deferred.**

Landed (all verified by the full suite + clippy):

- **`EqualityIndex::build` clone-on-hit** (`core/src/indexing/property_index.rs`):
  the build now does `get_mut` before `entry`, so it no longer allocates a key
  `String` or clones the `PropertyValue` for a bucket that already exists. The
  property-index pass visits every rule × property, so this removes the bulk of
  the build-time transient allocations. Build time in the memory grid dropped
  **120 ms → 96 ms**.
- **`Ruleset.ids` de-duplication** (`core/src/runtime/ruleset.rs:32`):
  `HashMap<Box<str>, RuleId>` instead of `HashMap<String, RuleId>` — removes
  per-key capacity slack and the extra word. `rule_id(&str)` still resolves via
  `Borrow<str>`.
- **`from_canonical` single parse** (`core/src/runtime/ruleset.rs`): the happy
  path now deserializes straight into `Vec<Rule>`, skipping the
  `serde_json::Value` DOM. A failure falls back to `from_canonical_slow`, which
  preserves the exact `SR_RULESET_CONSTRUCTION_FAILED`-naming-the-rule contract
  for a wrong-typed `priority` (ADR-0015) — the existing
  `from_canonical_rejects_wrong_typed_priority_naming_the_rule` and
  negative-priority tests exercise that fallback and stay green.

Measured (`bun run bench memory-scale --rules=100000 --vertices=10`): steady
state **78.7 → 77.8 MiB**, `bytes/rule` **825 → 816**. Small, as expected —
items 1/2 here are single-digit MB at 100k rules. The `from_canonical` win is
**not** covered by this grid (it builds from generated `Vec<Rule>`, not
canonical bytes), so it is rationale-verified, not measured.

Deferred to ticket **08**:

- `Ruleset.envelopes` is **not** a free removal: the R-tree is keyed by envelope
  (`(Rect, RuleId)` entries), not by `RuleId`, so the `Vec` is the O(1) by-id
  accessor behind `Ruleset::envelope` and the benchmark `RuleSource`, not a
  redundant copy.
- Ingestion's GeoJSON-DOM + `geo::Geometry` double representation needs a
  serde-direct ingest that preserves the edge matrix (BOM, missing id,
  unsupported types, numeric ids) — a real rewrite, not a trim.

Also noted: this run read `lifecycle.bounded: false` for one of the two
`bounded` flags (the ticket-03 run read both `true`). The verdict is flaky at
the margin; the measurement-rigor pass should pin down its heuristic.
