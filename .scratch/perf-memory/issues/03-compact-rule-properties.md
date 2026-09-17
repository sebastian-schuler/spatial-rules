# Memory at scale: compact rule-property storage

Type: task
Status: resolved

## Goal

Cut the per-rule property overhead, the largest single memory term at 100k rules.

## Problem

`Rule.properties` is `BTreeMap<String, PropertyValue>`
(`core/src/model/rule.rs:74`). A `BTreeMap`'s first insert allocates a leaf node
sized for 11 entries (~600 B with `String` + 32 B `PropertyValue`), whatever the
property count. The scaling generator gives every rule 2 properties, so much of
the documented 1.24 kB/rule is node padding.

## Plan

- Replace with an immutable-friendly sorted `Box<[(Box<str>, PropertyValue)]>`
  and a binary-search `get`. Rules are immutable after build, so ordered
  insertion at build time is enough.
- Adapt `RuleAccess::properties()` (`core/src/runtime/access.rs`) and the
  property-index build, which currently pass `&BTreeMap`.
- Check canonical (de)serialization ordering is preserved (`from_canonical`
  sorts by priority; `where` evaluation is order-independent).
- Evaluate boxing `PropertyValue::Str` (`core/src/model/properties.rs:17`) as
  part of the same pass.

## Acceptance

- `bytes/rule` drops in the memory-scale grid; build time not worse.
- `where` semantics, `$exists`/`$not`/`$nin`, and canonical round-trip
  byte-identical (proptest + `core/tests/query.rs`, `ruleset.rs` green).
- `docs/benchmarks.md` bytes/rule table re-recorded.

## Comments

**2026-09-16 (agent): resolved.**

- Added `Properties` (`core/src/model/properties.rs`): an immutable, key-sorted
  `Box<[(Box<str>, PropertyValue)]>` with `get`/`contains_key`/`iter`/`keys`/
  `len`/`is_empty`, a `from_pairs`/`FromIterator` constructor, a publication-time
  `insert(String, PropertyValue)`, and custom `serde` that round-trips as a JSON
  object (so canonical output is byte-identical to the `BTreeMap`, which was
  already key-sorted). `Rule.properties` is now `Properties`;
  `properties_from_json` returns it directly.
- Adapted `RuleAccess::properties`, `Ruleset::{properties, properties_checked}`,
  `WhereExpr::eval` (and its helpers), the property-index build, ingestion, the
  resolution value merge, the aggregate fold, and all test/benchmark builders.
- **Measured** (`bun run bench memory-scale --rules=100000 --vertices=10`):
  `bytes/rule` fell from **1300 B** (post-02, this session) / **1240 B**
  (documented 2026-08-23) to **825 B**; ruleset steady state **124.0 MiB →
  78.7 MiB**. The cell is also `lifecycle.bounded: true` again (it read `false`
  in the ticket-02 run — the lower footprint removed the transient pressure).
- Gates: `cargo test --workspace --exclude spatial-rules-python` green (all
  suites); `cargo clippy --workspace --all-targets --exclude spatial-rules-python`
  clean.
- `docs/benchmarks.md` §Memory gained a dated amendment table for the cell.

**Breaking change.** `Rule.properties` is a public field, so its type change
breaks `spatial-rules-core` consumers that read or build it as
`BTreeMap<String, PropertyValue>`. The engine's own read paths are unchanged
(`get`/`iter`/`len`); programmatic builders can use `Properties::insert` /
`from_pairs`. Needs a minor version bump under pre-1.0 semver — record in the
commit/changelog when this lands. `PropertyValue::Str` boxing (the optional
sub-item) was not done; it is worth ~8 B/value and can follow separately.
