# Compact rule-id tables: property index positions + de-duplicated id map

Type: task
Status: resolved

## Goal

Cut the two per-rule side tables that still carried redundant `RuleId` state:
`EqualityIndex`'s 16-byte ids and `Ruleset.ids`' duplicate copy of every id
string.

## Problem

Measured with a counting allocator at 100k rules × 10 vertices (2 properties per
rule), the retained side tables were **224.7 B/rule**:

| table | B/rule | waste |
|---|---|---|
| `EqualityIndex` (`Vec<RuleId>` grown by `push`) | 47.2 | 16 B per entry for an `owner` every entry shares; capacity-doubling slack |
| `Ruleset.ids` (`HashMap<Box<str>, RuleId>`) | 54.3 | a second heap copy of every id string + a 16 B value per key |

## Plan

- **`EqualityIndex`** (`core/src/indexing/property_index.rs`): store `u32`
  positions and the owning ruleset id, mint `RuleId::new(index, owner)` at the
  lookup seam, and `shrink_to_fit` every bucket after the build. `PropertyIndex`
  and its `HashSet<RuleId>` return type are unchanged.
- **`Ruleset.ids`** (`core/src/runtime/ruleset.rs`): replace the string-keyed map
  with `id_order: Vec<u32>` — positions sorted by id string. `rule_id` becomes a
  binary search (cold: the only production caller is exclusion mapping in
  `Ruleset::prepare`); duplicate detection keeps its declaration-order precedence
  via a transient `HashSet<&str>` borrow that is dropped before the ruleset is
  returned.

## Acceptance

- `bytes/rule` and ruleset steady state drop on the memory-scale grid.
- `where` semantics (`$eq`/`$in`/`$exists`/`$not`/`$nin`), the id round-trip, and
  duplicate rejection stay green; canonical output byte-identical.
- Steady-state throughput unchanged.

## Comments

**2026-09-17 (agent): resolved.**

`EqualityIndex` now stores `Vec<u32>` positions and the ruleset owner, and
`shrink_to_fit`s each bucket; `Ruleset.id_order` is a `Vec<u32>` sorted by id and
`rule_id` is a binary search. No public type changed (`PropertyIndex` is private,
`RuleId`/`Ruleset::rule_id` signatures are untouched).

**Measured** (`memory_scaling`, Windows; `100,000 × 10` three runs, `100,000 × 100`
one run):

| cell | steady ruleset | bytes/rule | after 1st query | build |
|---|---|---|---|---|
| Windows `100,000 × 10` before (post-04) | 77.9 MiB | 817 B | ~79 MiB | 93 ms |
| **Windows `100,000 × 10` after** | **67.3 MiB** | **706 B** | **68.1 MiB** | 97–100 ms |
| Linux `100,000 × 10` before (2026-08-23) | 120.8 MiB | 1.24 kB | 144.7 MiB | 129 ms |
| **Linux `100,000 × 10` after** | **71.6 MiB** | **0.75 kB** | **71.8 MiB** | 100 ms |
| Windows `100,000 × 100` before (2026-08-23) | 260.2 MiB | 2.73 kB | 285.8 MiB | 4.4 s |
| **Windows `100,000 × 100` after** | **209.6 MiB** | **2.20 kB** | **210.4 MiB** | ~4.7 s |
| Linux `100,000 × 100` before (2026-08-23) | 257.9 MiB | 2.64 kB | 282.0 MiB | 4.0 s |
| **Linux `100,000 × 100` after** | **208.8 MiB** | **2.19 kB** | **208.9 MiB** | 4.0 s |

Ticket-12 delta (Windows before/after): −10.6 MiB (−13.6%) / −111 B/rule at
10-vertex rings and −50.6 MiB (−19.4%) / −530 B/rule at 100-vertex rings. On the
Linux deploy platform the cumulative effect of tickets 02–04 + 12 is
120.8 → 71.6 MiB (−40.7%) and 257.9 → 208.8 MiB (−19.0%). Steady-state query
throughput is
28.3 M candidates/s — unchanged (the `intersects` mask fast path, perf-memory 05);
the edit is on the build and query-planning paths, not per candidate. Build reads
97–100 ms against the 93–106 ms recorded for this cell (no stable regression).

Gates: `cargo test -p spatial-rules-core --features benchmark` (all suites) and
`cargo clippy --workspace --all-targets --exclude spatial-rules-python` green.
`docs/benchmarks.md` §Memory gained a dated amendment for the cell.

**Known behaviour delta.** Duplicate-id detection now runs after the per-rule
validation pass instead of interleaved with it, so a ruleset containing *both* a
duplicate id and an invalid geometry can report the validation error rather than
the duplicate. No test or documented input combines the two; the error code and
the "names the rule" contract are unchanged, and the declaration-order duplicate
selection is preserved.

**Follow-up: dropped.** The remaining per-rule side tables are the spatial index
(~73 B/rule) and `envelopes` (~32 B/rule). A packed flat index that subsumes both
(~38–40 B/rule) was assessed and **dropped**: it saves ~65 B/rule (~9% at
100k×10) but only ~2.5% at realistic 100k×100, at the cost of rewriting the
per-candidate hot path, the `Ruleset::envelope` by-id contract, the lazy
`withinDistance` fringe index, and the `SpatialIndexKind` parity seam, plus an
ADR-0002 change. **Revisit only if** a load-shaped measurement at the target
ruleset size under the container cap shows the index is binding.

Also assessed and left: `Rule.id` → `Box<str>` (−8 B/rule, breaking public
field); the hoisted `priorities: Vec<i64>` (−8 B/rule, resolve-cache risk); and
property-value interning (only an interned 4-byte handle shrinks
`PropertyValue` — measured `String`/`Box<str>`/`Arc<str>` are all 24 B, so the
ticket-03 "boxing saves ~8 B/value" estimate is wrong).
