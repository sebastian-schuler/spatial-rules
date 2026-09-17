# Performance & memory workstream — Spec

**Status (2026-09-17): all tickets resolved.** Tickets 01–11 landed; ticket 12
(`issues/12-compact-id-tables.md`) added the final per-rule compaction —
100k×10 ruleset **71.6 MiB / 0.75 kB per rule** on Linux (67.3 MiB on Windows),
down from the 117.7 MiB / 1.24 kB baseline recorded below. The "Baseline" and
"Evidence driving the tickets" sections are the brief **as written on
2026-09-16**, retained for provenance: each item they list has since been
addressed or explicitly decided.

Created 2026-09-16. Supersedes the cancelled geo 0.34 prepared-geometry-sharing
idea (`.scratch/post-v1/issues/05-geo-034-upgrade.md`, deleted): geo's
`PreparedGeometry` is `!Send + !Sync` in geo 0.33 (its `GeometryGraph` holds an
`Rc`), so the thread-local memo is the design, not a placeholder (ADR-0010).
This effort pursues the remaining headroom **in our own code**.

## Baseline (see `docs/benchmarks.md`)

- **Latency** — production 1,000 candidates × 30 rules ≈ **15 ms core / 18.5 ms
  addon**; turf ≈ 60× slower. The dominant lever is prepared geometry
  (B→E ≈ 31×); the bbox index is negligible at 30 rules.
- **Memory** — 1.24–2.73 kB/rule. 100k×10 ruleset 117.7 MiB, "after 1st query"
  143.3 MiB. 100k candidates ≈ 456 ms, parse-dominated.

## Axes

1. **Measurement integrity + build config** (foundation) — ticket 01.
2. **Memory at scale (100k rules)** — tickets 02, 03, 04.
3. **Hot path / feature surfaces** — tickets 05, 06.
4. **Boundary & serialization** — ticket 07.

Plus ticket 08 — deferred from 04: `Ruleset.envelopes`' by-id copy and
single-representation ingestion (both need a design change, not a trim).

## Sequencing

Ticket 01 first: it makes every later number trustworthy and is a free broad
speedup. Then 02–04 (memory) and 05–07 (latency/boundary) are independent.

## Evidence driving the tickets

- **The prepared memo is O(rules), not O(touched).** `Ruleset::prepare` always
  calls `PreparedMemo::for_ruleset`, which allocates a dense
  `Vec<Option<PreparedGeometry>>` sized to the rule count
  (`core/src/indexing/prepared_cache.rs:74`). On the unprepared `intersects`
  mask fast path no slot is ever filled, yet the vector is still allocated —
  ≈25 MiB at 100k rules for zero prepared geometries, which is exactly the
  measured "after 1st query" margin (143.3 − 117.7 = 25.6 MiB). The
  `docs/benchmarks.md` footnote describes this margin as workload-dependent
  ("grows with the rules the candidates touch"); it does not.
- **`Rule.properties` is a `BTreeMap`** (`core/src/model/rule.rs:74`): the first
  insert allocates an 11-entry leaf (~600 B) per rule regardless of property
  count — the scaling generator gives every rule 2 properties.
- **Ladder `build_30_rules` clones the rules inside the measured closure**
  (`benchmarks/benches/ladder.rs:158`), inflating the reported build time.
- **No `[profile.release]` tuning** anywhere in the workspace.
- Duplicate per-rule state at build (`Ruleset.envelopes`, `Ruleset.ids`,
  `EqualityIndex` clone-on-hit) and double-representation at ingestion
  (GeoJSON DOM + `geo::Geometry`; `from_canonical` two-pass `Value`).
- Rich JSON is built as a `serde_json::Value` DOM then stringified
  (`bindings-common/src/lib.rs`); `queryAsync` copies the whole candidate Buffer
  (`node/src/lib.rs:94`).

## Non-goals

- Anything requiring cross-thread sharing of `PreparedGeometry` (ADR-0010).
- Changing predicate semantics, the mask, or the wire contract.

## Verification

Every ticket re-runs the affected harness and records the delta in
`docs/benchmarks.md`:

- **Memory** — `bun run bench memory-scale` (RSS is the evidence).
- **Timing** — `bun run bench samples` (perf-memory 01): runs the criterion bench
  N times and reports the median with the run-to-run spread, so a delta is
  judged against its noise floor, not a stale `target/criterion` baseline. Use
  `--save=`/`--compare=` for an A/B and `--no-lto` to test the release profile.
- **Throughput/serving** — `bun run bench perf` / `bun run bench memory`.

`cargo test --workspace --exclude spatial-rules-python` + clippy must stay green.
