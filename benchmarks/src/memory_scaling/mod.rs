//! Memory scaling and lifecycle benchmark (memory-benchmark ticket 01).
//!
//! Answers: how does memory scale with rules and geometry complexity, and
//! does anything leak across the ruleset lifecycle? No behavior change —
//! measurement code only, driving the core's public seams:
//!
//! - **Build vs steady-state vs query-time** measured separately: peak RSS
//!   during index construction (`Ruleset::build`), resident footprint after
//!   the generator inputs are dropped, and allocation behavior under repeated
//!   batch queries.
//! - **Scaling grid**: rule counts × vertices per polygon, reporting index
//!   bytes, bytes/rule, bytes/vertex — whether memory tracks rule count or
//!   coordinate count.
//! - **Lifecycle check**: repeated atomic replacement through
//!   [`spatial_rules_core::Engine`] (ADR-0007 swap path), one query per swap
//!   to exercise the per-thread prepared-geometry memo (ADR-0010); RSS after
//!   the first vs the last replacement detects retention.
//! - **Ground truth is process-level RSS**, not allocator or heap numbers —
//!   VmRSS/VmHWM from `/proc/self/status` on Linux, working-set counters from
//!   `GetProcessMemoryInfo` on Windows — consistent with the method recorded
//!   in `docs/benchmarks.md` §Memory.
//!
//! Each scale cell runs in its own child process (re-exec via
//! `std::env::current_exe`) so every cell sees a clean baseline; the parent
//! aggregates the per-cell JSON reports.
//!
//! The module is split by concern: [`report`] holds the report/config types,
//! [`heuristics`] holds the pure boundedness and per-unit measurements,
//! [`generator`] holds the deterministic geometry generation, and this file
//! holds the [`measure_cell`] orchestration plus the tests.

mod generator;
mod heuristics;
mod report;

use std::sync::atomic::{AtomicBool, Ordering};
use std::time::Instant;

use spatial_rules_core::{Engine, Query, SpatialError, SpatialPredicate};

use generator::{generate_candidates, generate_rules};
use heuristics::{
    bytes_per_million_vertices, bytes_per_rule, bytes_per_vertex, is_bounded, is_bounded_series,
};

pub use report::{CellOptions, CellReport, LifecycleReport, QueryTimeReport, Scale};

/// Per-cell lifecycle wall-time budget (seconds). The default battery keeps
/// every cell inside this envelope; cells whose per-swap cost would blow past
/// it get their replacement count floored at [`MIN_LIFECYCLE_SWAPS`] instead
/// of running for hours (memory-benchmark ticket 01 "stuck" symptom: the
/// 100k×1k cell is ~8 min per build).
pub const LIFECYCLE_BUDGET_SECS: f64 = 120.0;

/// Never run a lifecycle phase below this many swaps — first-vs-last spread
/// still detects a gross retention with two, and the series test needs ≥4.
pub const MIN_LIFECYCLE_SWAPS: usize = 2;

/// Fitted per-swap build cost: strict validation (ADR-0005) is quadratic in
/// per-ring vertex count. Measured ≈4.7 s per 10^9 `rules·vertices²` units
/// (the 1k rules × 1k vertices cell); used only to bound the default grid.
const BUILD_UNITS_PER_SEC: f64 = 1e9 / 4.7;

/// Cap a requested replacement count so the cell's lifecycle phase stays
/// within [`LIFECYCLE_BUDGET_SECS`]. The cap is a budget heuristic, never
/// below [`MIN_LIFECYCLE_SWAPS`] and never above `requested`.
pub fn capped_replacements(scale: Scale, requested: usize) -> usize {
    let units = scale.rules as f64 * (scale.vertices as f64).powi(2);
    let secs = units / BUILD_UNITS_PER_SEC;
    let budget = (LIFECYCLE_BUDGET_SECS / secs.max(1e-6)) as usize;
    budget.clamp(MIN_LIFECYCLE_SWAPS, requested.max(MIN_LIFECYCLE_SWAPS))
}

/// Progress logging goes to stderr (stdout carries the JSON report); enabled
/// by the binary so long cells don't look stuck.
static PROGRESS: AtomicBool = AtomicBool::new(false);

/// Enable phase progress lines on stderr.
pub fn set_progress(enabled: bool) {
    PROGRESS.store(enabled, Ordering::Relaxed);
}

fn note(message: &str) {
    if PROGRESS.load(Ordering::Relaxed) {
        eprintln!("[memory-scaling] {message}");
    }
}

fn snapshot() -> crate::rss::Snapshot {
    crate::rss::snapshot().unwrap_or_else(crate::rss::Snapshot::zero)
}

/// Measure one scale cell end-to-end. Runs in a dedicated child process in
/// aggregate mode; calling it repeatedly in one process inflates later cells'
/// baselines with earlier cells' retained memory.
pub fn measure_cell(scale: Scale, opts: CellOptions) -> Result<CellReport, SpatialError> {
    let start = snapshot();

    note(&format!(
        "generating {} rules x {} vertices...",
        scale.rules, scale.vertices
    ));
    let rules = generate_rules(scale);
    let candidates = generate_candidates(opts.candidates, scale);
    let after_generation = snapshot();
    let generation_peak_delta = after_generation.peak.saturating_sub(start.peak);

    note("building ruleset (validation + envelopes + rstar + property index)...");
    let build_started = Instant::now();
    let engine = Engine::new(rules)?;
    let build_duration = build_started.elapsed();
    let after_build = snapshot();
    let build_peak_delta = after_build.peak.saturating_sub(after_generation.peak);
    let steady_state_delta = after_build.rss.saturating_sub(start.rss);
    note(&format!(
        "build done in {} ms; steady-state delta {:.1} MiB",
        build_duration.as_millis(),
        steady_state_delta as f64 / 1024.0 / 1024.0
    ));

    // ---- query-time phase ----
    // Cold batch first (fills this thread's touched-rule prepared geometries),
    // excluded from the steady-state timing like every other harness
    // (ADR-0010).
    let query = Query::new(SpatialPredicate::Intersects);
    let cold_started = Instant::now();
    let _mask = engine.query_mask(&candidates, &query);
    let cold_duration = cold_started.elapsed();

    let timed_started = Instant::now();
    let mut rss_samples = Vec::with_capacity(opts.query_batches);
    for _ in 0..opts.query_batches {
        let _mask = engine.query_mask(&candidates, &query);
        rss_samples.push(snapshot().rss);
    }
    let timed_duration = timed_started.elapsed();
    note(&format!(
        "query phase done ({} batches x {} candidates)",
        opts.query_batches, opts.candidates
    ));
    let queries_per_sec = if timed_duration.as_secs_f64() > 0.0 {
        (opts.query_batches * opts.candidates) as f64 / timed_duration.as_secs_f64()
    } else {
        0.0
    };
    let rss_first = rss_samples[0];
    let rss_last = rss_samples[rss_samples.len() - 1];
    // ---- lifecycle phase: repeated atomic replacement (ADR-0007) ----
    let pre_lifecycle_peak = snapshot().peak;
    let lifecycle_started = Instant::now();
    let mut rss_after_each = Vec::with_capacity(opts.replacements);
    let mut commit_after_each = Vec::with_capacity(opts.replacements);
    for index in 0..opts.replacements {
        note(&format!(
            "lifecycle replacement {}/{}...",
            index + 1,
            opts.replacements
        ));
        let fresh = generate_rules(scale);
        engine.replace(fresh)?;
        // One query per swap evicts the stale thread-local prepared set and
        // fills the new ruleset's touched rules (ADR-0010) — retention would
        // show up here.
        let _mask = engine.query_mask(&candidates, &query);
        let snap = snapshot();
        rss_after_each.push(snap.rss);
        commit_after_each.push(snap.commit);
    }
    let lifecycle_duration = lifecycle_started.elapsed();
    let lifecycle_first = rss_after_each[0];
    let lifecycle_last = rss_after_each[rss_after_each.len() - 1];

    Ok(CellReport {
        scale,
        total_vertices: scale.total_vertices(),
        baseline_rss_bytes: start.rss,
        baseline_peak_bytes: start.peak,
        generation_peak_delta_bytes: generation_peak_delta,
        build_duration_ms: build_duration.as_millis(),
        build_peak_delta_bytes: build_peak_delta,
        steady_state_delta_bytes: steady_state_delta,
        bytes_per_rule: bytes_per_rule(steady_state_delta, scale.rules),
        bytes_per_vertex: bytes_per_vertex(steady_state_delta, scale.total_vertices()),
        bytes_per_million_vertices: bytes_per_million_vertices(
            steady_state_delta,
            scale.total_vertices(),
        ),
        query_time: QueryTimeReport {
            first_batch_ms: cold_duration.as_millis(),
            batches: opts.query_batches,
            candidates_per_batch: opts.candidates,
            queries_per_sec,
            rss_first_bytes: rss_first,
            rss_last_bytes: rss_last,
            bounded: is_bounded_series(&rss_samples).unwrap_or_else(|| is_bounded(rss_first, rss_last)),
        },
        lifecycle: LifecycleReport {
            replacements: opts.replacements,
            rss_after_first_bytes: lifecycle_first,
            rss_after_last_bytes: lifecycle_last,
            spread_bytes: lifecycle_last as i64 - lifecycle_first as i64,
            bounded: is_bounded_series(&rss_after_each)
                .unwrap_or_else(|| is_bounded(lifecycle_first, lifecycle_last)),
            rss_after_each_bytes: rss_after_each,
            commit_after_each_bytes: commit_after_each,
            lifecycle_peak_delta_bytes: snapshot()
                .peak
                .saturating_sub(pre_lifecycle_peak),
            replace_build_ms_total: lifecycle_duration.as_millis(),
        },
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use spatial_rules_core::Ruleset;

    #[test]
    fn generates_exactly_the_requested_rule_count() {
        let scale = Scale {
            rules: 37,
            vertices: 10,
        };
        assert_eq!(generate_rules(scale).len(), 37);
    }

    #[test]
    fn every_exterior_ring_has_exactly_the_requested_vertex_count() {
        let scale = Scale {
            rules: 5,
            vertices: 100,
        };
        for rule in generate_rules(scale) {
            let geo::Geometry::MultiPolygon(multi) = &rule.geometry else {
                panic!("expected MultiPolygon");
            };
            assert_eq!(multi.0.len(), 1, "one part per rule");
            let exterior = multi[0].exterior();
            assert_eq!(exterior.0.len(), 101, "vertices + closing repeat");
        }
    }

    #[test]
    fn generation_is_deterministic_for_a_given_scale() {
        let scale = Scale {
            rules: 8,
            vertices: 10,
        };
        let first = generate_rules(scale);
        let second = generate_rules(scale);
        for (a, b) in first.iter().zip(second.iter()) {
            assert_eq!(a.id, b.id);
            match (&a.geometry, &b.geometry) {
                (
                    geo::Geometry::MultiPolygon(ma),
                    geo::Geometry::MultiPolygon(mb),
                ) => {
                    let ka = ma[0].exterior().0.clone();
                    let kb = mb[0].exterior().0.clone();
                    assert_eq!(ka, kb);
                }
                _ => panic!("expected MultiPolygons"),
            }
        }
    }

    #[test]
    fn generated_rules_build_a_valid_ruleset_at_every_grid_size() {
        for vertices in [10usize, 100] {
            let scale = Scale {
                rules: 50,
                vertices,
            };
            Ruleset::build(generate_rules(scale))
                .expect("generated rules must pass strict validation");
        }
    }

    #[test]
    fn generates_the_requested_candidate_count_deterministically() {
        let scale = Scale {
            rules: 30,
            vertices: 10,
        };
        let first = generate_candidates(20, scale);
        let second = generate_candidates(20, scale);
        assert_eq!(first.len(), 20);
        for (a, b) in first.iter().zip(second.iter()) {
            assert_eq!(a.id, b.id);
        }
    }

    #[test]
    fn bounded_when_last_sits_within_tolerance_of_first() {
        // ±5% of the first observation is wobble, not a leak.
        assert!(is_bounded(100_000, 104_900));
        assert!(is_bounded(100_000, 95_100));
        assert!(is_bounded(100_000, 100_000));
    }

    #[test]
    fn unbounded_when_last_climbs_past_tolerance() {
        assert!(!is_bounded(100_000, 105_200));
        // A doubling is a leak, and a climb from zero cannot be bounded.
        assert!(!is_bounded(1, 512 * 1024));
        assert!(!is_bounded(0, 1));
    }

    #[test]
    fn series_with_warmup_climb_then_plateau_is_bounded() {
        // Shape of the real 1k×10 lifecycle trace: climbs ~2 MiB over the
        // first swaps (arena/cache warmup), then plateaus with wobble.
        let samples: Vec<u64> = [
            [10.6, 11.2, 11.5, 12.7, 12.3, 12.4].as_slice(),
            &[13.4, 12.8, 12.3, 12.3, 13.3, 12.4, 13.2, 12.1],
            &[13.4, 12.7, 12.6, 12.2, 12.6, 12.7],
        ]
        .concat()
        .iter()
        .map(|mib| (*mib * 1024.0 * 1024.0) as u64)
        .collect();
        assert_eq!(is_bounded_series(&samples), Some(true));
    }

    #[test]
    fn series_with_a_steady_per_swap_leak_is_unbounded() {
        let leaky: Vec<u64> = (0..20)
            .map(|index| 10_000_000u64 + index as u64 * 200_000)
            .collect();
        assert_eq!(is_bounded_series(&leaky), Some(false));
    }

    #[test]
    fn warmup_step_then_flat_is_bounded() {
        // Real 10000x10 Linux lifecycle trace (memory-benchmark ticket 03): a
        // one-time ~7 MiB step over the first swaps, then flat — warmup, not
        // a leak. The old quarter-mean test read the tail as drift.
        let samples: Vec<u64> = [
            26, 28, 30, 31, 31, 31, 31, 31, 31, 31, 31, 31, 31, 33, 33, 33, 33, 33, 33, 33,
        ]
        .iter()
        .map(|mib| mib * 1024 * 1024)
        .collect();
        assert_eq!(is_bounded_series(&samples), Some(true));
    }

    #[test]
    fn sawtooth_returning_to_its_floor_is_bounded() {
        // Real 100000x10 Linux 50-swap probe (memory-benchmark ticket 03):
        // ~21 MiB per-swap climb, then glibc trims back to the same ~286 MiB
        // floor every ~11-18 swaps. (Trace values differ slightly from the
        // 20-swap grid run below — separate runs.)
        let samples: Vec<u64> = [
            224, 245, 285, 286, 287, 287, 287, 287, 308, 329, 350, 372, 393, 414, 436, 457,
            478, 500, 521, 543, 286, 307, 328, 350, 286, 307, 328, 350, 371, 392, 414, 435,
            456, 478, 499, 520, 542, 563, 286, 307, 328, 350, 286, 307, 328, 286, 307, 328,
            286, 307,
        ]
        .iter()
        .map(|mib| mib * 1024 * 1024)
        .collect();
        assert_eq!(is_bounded_series(&samples), Some(true));
    }

    #[test]
    fn sawtooth_with_a_reset_in_window_is_bounded_at_20_swaps() {
        // Real 100000x100 Linux 20-swap trace: climbs, then a trim drops it
        // back to ~562 — a reset is visible inside the window, so the verdict
        // must not read the up-slope as a leak.
        let samples: Vec<u64> = [
            499, 521, 560, 582, 605, 605, 562, 562, 583, 583, 561, 583, 583, 561, 582, 604,
            625, 646, 668, 582,
        ]
        .iter()
        .map(|mib| mib * 1024 * 1024)
        .collect();
        assert_eq!(is_bounded_series(&samples), Some(true));
    }

    #[test]
    fn upslope_with_no_reset_in_window_is_inconclusive() {
        // Real 100000x10 Linux 20-swap grid trace: an up-slope with no reset
        // inside the window — the verdict must stay `false` (the honest answer
        // is "run the 50-swap probe", which then proves the sawtooth bounded).
        let samples: Vec<u64> = [
            224, 246, 285, 286, 287, 287, 287, 287, 308, 329, 350, 372, 393, 415, 436, 457,
            479, 500, 521, 543,
        ]
        .iter()
        .map(|mib| mib * 1024 * 1024)
        .collect();
        assert_eq!(is_bounded_series(&samples), Some(false));
    }

    #[test]
    fn slow_leak_is_still_unbounded_at_the_20_swap_window() {
        // A leak of ~1.5% of the footprint per swap at the real 20-swap window:
        // slow enough that its 5-sample tail is nearly flat, so a loose plateau
        // tolerance would rescue it as "bounded" — the exact hole a 2%
        // PLATEAU_RANGE_RATIO closes (a 5-sample tail spans 4 intervals, so it
        // still rejects any leak above ~0.5%/swap, matching the steady test).
        let samples: Vec<u64> = (0..20).map(|i| 10_000_000 + i * 150_000).collect();
        assert_eq!(is_bounded_series(&samples), Some(false));
    }

    #[test]
    fn tail_plateau_catches_a_late_warmup_step() {
        // Fresh 10000x10 Linux trace (memory-benchmark ticket 03 re-run): the
        // warmup step is gradual (26 -> 35 MiB) and finishes late, so quarter
        // means still drift — but the last quarter is flat at 35, which is the
        // no-leak signal (a genuine leak never flattens).
        let samples: Vec<u64> = [
            26, 28, 30, 30, 31, 31, 31, 31, 31, 31, 31, 31, 31, 33, 35, 35, 35, 35, 35, 35,
        ]
        .iter()
        .map(|mib| mib * 1024 * 1024)
        .collect();
        assert_eq!(is_bounded_series(&samples), Some(true));
    }

    #[test]
    fn series_needs_four_samples_to_make_a_claim() {
        assert_eq!(is_bounded_series(&[]), None);
        assert_eq!(is_bounded_series(&[1, 2, 3]), None);
        // Four samples form single-point quarters: flat stays bounded, a
        // strict climb does not.
        assert_eq!(is_bounded_series(&[5, 5, 5, 5]), Some(true));
        assert_eq!(is_bounded_series(&[1, 2, 3, 4]), Some(false));
    }

    #[test]
    fn per_unit_ratios_divide_the_delta() {
        assert_eq!(bytes_per_rule(3000, 3), Some(1000.0));
        assert_eq!(bytes_per_vertex(3000, 150), Some(20.0));
        assert_eq!(
            bytes_per_million_vertices(3000, 150),
            Some(20_000_000.0)
        );
    }

    #[test]
    fn per_unit_ratios_are_undefined_for_zero_divisors() {
        assert_eq!(bytes_per_rule(3000, 0), None);
        assert_eq!(bytes_per_vertex(3000, 0), None);
        assert_eq!(bytes_per_million_vertices(3000, 0), None);
    }

    #[test]
    fn replacement_cap_keeps_small_cells_at_requested_count() {
        let scale = Scale {
            rules: 1_000,
            vertices: 10,
        };
        assert_eq!(capped_replacements(scale, 20), 20);
    }

    #[test]
    fn replacement_cap_floors_the_lifecycle_on_quadratic_cells() {
        // 10000×1000 (10M vertices) builds in ~47 s/swap: 20 swaps would take
        // ~16 min. The budget floors it at the 2-swap minimum rather than
        // letting the default grid run for hours.
        let scale = Scale {
            rules: 10_000,
            vertices: 1_000,
        };
        assert_eq!(capped_replacements(scale, 20), 2);
        // A tiny request is never pushed below the minimum.
        assert_eq!(capped_replacements(scale, 0), 2);
    }
}
