# Measurement integrity + release profile

Type: task
Status: resolved

## Goal

Make the baseline trustworthy and take the free broad speedup, before optimising
against numbers we can't trust.

## Plan

- `benchmarks/benches/ladder.rs:158` — `black_box(rules.clone())` is inside the
  measured closure, so `ruleset_build/build_30_rules` times a full rule clone.
  Hoist the clone out (clone once, then measure `Ruleset::build`).
- `benchmarks/src/memory_scaling/mod.rs:125` — the default cell drives
  `query_mask` + `intersects`, which never prepares a rule, so the "after 1st
  query" margin measures the (unfilled) slot vector, not the lazy memo. Either
  add a rich/resolve batch that actually exercises preparation, or split the
  reported cell so the two costs are separated. See perf-memory ticket 02 —
  the fix there changes what this cell should show.
- Add `[profile.release]` to the root `Cargo.toml`: `lto = "thin"`,
  `codegen-units = 1`. Assess `panic = "abort"` separately (napi/PyO3
  unwinding across FFI, `should_panic` tests) — do not bundle it here.
- Re-run the affected harnesses and re-record `docs/benchmarks.md`.

## Acceptance

- `build_30_rules` no longer includes the rule clone.
- Release profile is set; build time cost of LTO noted.
- `docs/benchmarks.md` Summary/Results re-recorded with the profile change
  called out, and the memory-footnote wording corrected.

## Comments

**2026-09-16 (agent): first half landed.**

- `[profile.release]` added to the root `Cargo.toml`: `lto = "thin"`,
  `codegen-units = 1`. `panic = "abort"` deliberately omitted (napi/PyO3 cross
  the FFI boundary). `cargo build --release -p spatial-rules-benchmarks` links
  clean (29.6 s).
- `benchmarks/benches/ladder.rs`: `build_30_rules` now uses `iter_batched`, so
  the per-iteration `rules.clone()` (required because `Ruleset::build` consumes
  its input) runs outside the timed region.
- **Measurement caveat (important).** A single criterion run is dominated by
  run-to-run noise and cannot isolate the LTO delta: this run reports
  E 14.0 ms / F 13.1 ms / build 23.1 ms / prepare 4.9 ms, while criterion's
  change-vs-baseline shows **+14–19%** against a *stale local* baseline from
  `target/criterion` (unknown profile/machine). The absolute `build` figure sits
  inside the documented 14–33 ms spread. Do not re-record `docs/benchmarks.md`
  from this run.
- **Still open:** a controlled re-baseline (same session, both profiles,
  medians-of-N) and the `docs/benchmarks.md` re-record + footnote correction.
  The memory-grid change (exercise the prepared memo rather than the unprepared
  `intersects` fast path) is folded into ticket 02, which changes what that cell
  should report.

**2026-09-16 (agent): resolved — controlled harness built, LTO measured.**

- Added `bun run bench samples` (`bench.mjs` + a `samples` section in
  `benchmarks.json`). It runs the criterion bench N times with the
  machine-readable `bencher` output, reports the **median and the run-to-run
  spread** per bench, and can `--save=`/`--compare=` a JSON summary so an A/B is
  judged as "does the delta exceed the wider spread?" rather than against
  criterion's stale `target/criterion` baseline. Defaults: 5 runs, warm-up 1 s,
  measure 2 s, sample-size 10; `--filter=` passes a criterion filter.
- **Measured A/B on the production hot path** (`batch_query/F_prepared_rstar_bbox`,
  5 runs each, same session):

  | config | median | spread |
  |---|---|---|
  | `lto = "thin"`, `codegen-units = 1` | **10.918 ms** | ±4.7% |
  | `lto = false`, `codegen-units = 16` | 12.249 ms | ±3.9% |

  **+12.2% slower without LTO**, which exceeds the ±4.7% noise floor — so thin
  LTO is a real ~12% win. (The no-LTO side was produced via
  `CARGO_PROFILE_BENCH_LTO=false CARGO_PROFILE_BENCH_CODEGEN_UNITS=16`, no
  manifest edit.)
- `docs/benchmarks.md` ladder Results gained a dated amendment recording the
  above, and the memory footnote was corrected (ticket 02).
- The memory-grid "exercise the prepared memo" change is covered by ticket 02's
  outcome (the cell now measures the boxed slot table).

Acceptance met. Note the absolute ladder figures in the docs predate both the
profile change and this harness; a full re-record is a separate pass if wanted.
