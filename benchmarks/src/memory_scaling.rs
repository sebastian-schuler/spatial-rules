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

use std::collections::BTreeMap;
use std::time::Instant;

use geo::{Coord, LineString, MultiPolygon, Polygon};
use serde::{Deserialize, Serialize};
use spatial_rules_core::{Candidate, PropertyValue, Rule};
use spatial_rules_core::{SpatialError};

/// One cell of the scaling grid: rule count × vertices per exterior ring.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
pub struct Scale {
    /// Number of rules in the generated ruleset.
    pub rules: usize,
    /// Vertices per rule's exterior ring (exact, no holes).
    pub vertices: usize,
}

impl Scale {
    /// Total coordinates ingested across all rules (exterior rings only;
    /// each ring repeats its first vertex as the closing point).
    pub fn total_vertices(&self) -> usize {
        self.rules * self.vertices
    }
}

/// Knobs for one measurement cell.
#[derive(Debug, Clone, Copy)]
pub struct CellOptions {
    /// Candidates per query batch.
    pub candidates: usize,
    /// Query batches for the query-time phase.
    pub query_batches: usize,
    /// Atomic replacements for the lifecycle phase.
    pub replacements: usize,
}

impl Default for CellOptions {
    fn default() -> Self {
        CellOptions {
            candidates: 1000,
            query_batches: 20,
            replacements: 20,
        }
    }
}

// ---- pure seams (unit-tested below; no process state touched) --------------

/// A relative tolerance for the boundedness classification: resident memory
/// may legitimately wobble by this fraction of the steady-state footprint
/// (allocator retention, page trimming) without indicating a leak.
pub const BOUNDED_TOLERANCE_RATIO: f64 = 0.05;

/// Whether resident memory is *bounded* across a repeated operation: the
/// spread between the last and first observation must sit within
/// [`BOUNDED_TOLERANCE_RATIO`] of the **first** observation. A leak makes
/// `last` climb monotonically past any such tolerance.
pub fn is_bounded(first_bytes: u64, last_bytes: u64) -> bool {
    let spread = last_bytes.abs_diff(first_bytes) as f64;
    spread <= BOUNDED_TOLERANCE_RATIO * first_bytes as f64
}

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

/// Whether a *series* of observations is bounded: the drift between the
/// mean of the second quarter and the mean of the fourth quarter must sit
/// within [`BOUNDED_TOLERANCE_RATIO`]. Comparing quarter means (not endpoints)
/// ignores one-off warmup climbs (allocator arenas, thread-local caches that
/// fill on the first swaps) while still catching a steady per-swap leak.
///
/// Returns `None` when there are fewer than 4 observations — not enough to
/// form quarters, so no claim is made.
pub fn is_bounded_series(samples: &[u64]) -> Option<bool> {
    if samples.len() < 4 {
        return None;
    }
    let quarter = samples.len() / 4;
    let mean = |slice: &[u64]| -> f64 {
        slice.iter().map(|&value| value as f64).sum::<f64>() / slice.len() as f64
    };
    let early = mean(&samples[quarter..2 * quarter]);
    let late = mean(&samples[3 * quarter..]);
    Some((late - early).abs() <= BOUNDED_TOLERANCE_RATIO * early.max(1.0))
}

/// Steady-state bytes attributable to one rule.
///
/// Returns `None` when `rules == 0` — the ratio is undefined, not zero.
pub fn bytes_per_rule(steady_state_delta_bytes: u64, rules: usize) -> Option<f64> {
    if rules == 0 {
        return None;
    }
    Some(steady_state_delta_bytes as f64 / rules as f64)
}

/// Steady-state bytes attributable to one ingested coordinate.
///
/// Returns `None` when `total_vertices == 0`.
pub fn bytes_per_vertex(steady_state_delta_bytes: u64, total_vertices: usize) -> Option<f64> {
    if total_vertices == 0 {
        return None;
    }
    Some(steady_state_delta_bytes as f64 / total_vertices as f64)
}

/// The headline publishable metric: steady-state bytes per **million**
/// vertices (the number someone sizing a container asks for).
pub fn bytes_per_million_vertices(
    steady_state_delta_bytes: u64,
    total_vertices: usize,
) -> Option<f64> {
    if total_vertices == 0 {
        return None;
    }
    let per_vertex = bytes_per_vertex(steady_state_delta_bytes, total_vertices)?;
    Some(per_vertex * 1_000_000.0)
}

// ---- measurement phases -----------------------------------------------------

use std::sync::atomic::{AtomicBool, Ordering};

use spatial_rules_core::{Engine, Query, SpatialPredicate};

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

/// Byte-level measurements for one scale cell. All byte values are
/// process-level resident ground truth ([`crate::rss`]); each cell runs in
/// its own child process so peaks measure that cell alone.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CellReport {
    pub scale: Scale,
    pub total_vertices: usize,
    /// Resident footprint and all-time peak before anything is generated.
    pub baseline_rss_bytes: u64,
    pub baseline_peak_bytes: u64,
    /// Peak overhead while generating the rule list (transient input).
    pub generation_peak_delta_bytes: u64,
    pub build_duration_ms: u128,
    /// Peak overhead during index construction (validation, envelopes,
    /// rstar bulk load, property index).
    pub build_peak_delta_bytes: u64,
    /// Steady-state delta over baseline after the generated inputs are moved
    /// into the ruleset and transients are freed — the resident footprint of
    /// rules + envelopes + indexes. An upper bound: allocators may retain
    /// freed transients.
    pub steady_state_delta_bytes: u64,
    pub bytes_per_rule: Option<f64>,
    pub bytes_per_vertex: Option<f64>,
    /// Headline metric: steady-state bytes per million vertices.
    pub bytes_per_million_vertices: Option<f64>,
    pub query_time: QueryTimeReport,
    pub lifecycle: LifecycleReport,
}

/// Allocation behavior under repeated batch queries.
#[derive(Debug, Clone, Copy, Serialize, Deserialize)]
pub struct QueryTimeReport {
    /// Time of the first (cold) batch — includes the touched rules'
    /// prepared-geometry fills for this thread (ADR-0010). Lazily prepared, so
    /// it tracks the rules the candidates touch, not the whole ruleset.
    pub first_batch_ms: u128,
    pub batches: usize,
    pub candidates_per_batch: usize,
    /// Steady-state throughput across the remaining batches.
    pub queries_per_sec: f64,
    pub rss_first_bytes: u64,
    pub rss_last_bytes: u64,
    pub bounded: bool,
}

/// Retention across repeated atomic ruleset replacement (ADR-0007 swap path),
/// one query per swap to exercise the per-thread prepared-geometry memo
/// eviction (ADR-0010).
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct LifecycleReport {
    pub replacements: usize,
    pub rss_after_first_bytes: u64,
    pub rss_after_last_bytes: u64,
    pub spread_bytes: i64,
    pub bounded: bool,
    /// Resident set after every swap — lets a reader tell a monotonic
    /// climb (leak) from one-off wobble (allocator retention).
    pub rss_after_each_bytes: Vec<u64>,
    /// Committed (private) bytes after every swap. The leak discriminator:
    /// resident memory can climb while pages are merely freed-but-resident;
    /// commit charge falls on real frees. Flat commit under climbing RSS
    /// ⇒ allocator retention, not a leak.
    pub commit_after_each_bytes: Vec<u64>,
    /// Extra peak the whole lifecycle added over the pre-lifecycle peak —
    /// captures old + in-build-new coexisting mid-swap plus the fresh
    /// thread-local prepared geometries.
    pub lifecycle_peak_delta_bytes: u64,
    /// Total build time across all replacement builds.
    pub replace_build_ms_total: u128,
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

// ---- deterministic generator ------------------------------------------------

/// Deterministic star-shaped ring around `(cx, cy)` with exactly `vertices`
/// distinct points plus the closing repeat. Positive radius at every angle
/// keeps the ring simple, so the polygon passes strict validation (ADR-0005).
fn ring(rng: &mut crate::dataset::Rng, cx: f64, cy: f64, vertices: usize) -> LineString<f64> {
    let mut coords = Vec::with_capacity(vertices + 1);
    for index in 0..vertices {
        let angle = (index as f64 / vertices as f64) * std::f64::consts::TAU;
        // Jitter keeps the shape irregular but always positive-radius
        // (star-shaped ⇒ simple ⇒ valid), mirroring dataset.rs's blobs.
        let radius = 1.0 + 0.45 * rng.f64();
        coords.push(Coord {
            x: cx + radius * angle.cos(),
            y: cy + radius * angle.sin(),
        });
    }
    coords.push(coords[0]);
    LineString::from(coords)
}

/// Generate `scale.rules` valid MultiPolygon rules laid out on a coarse grid
/// so envelopes don't fully overlap (the rstar index must be able to prune).
/// Deterministic for a given [`Scale`]: same seed, same layout, same shapes.
pub fn generate_rules(scale: Scale) -> Vec<Rule> {
    let mut rng = crate::dataset::Rng::new(0x5EED_1A2B_3C4D);
    let columns = (scale.rules as f64).sqrt().ceil() as usize;
    let pitch = 10.0_f64;
    (0..scale.rules)
        .map(|index| {
            let column = (index % columns.max(1)) as f64;
            let row = (index / columns.max(1)) as f64;
            let cx = column * pitch;
            let cy = row * pitch;

            let mut properties = BTreeMap::new();
            properties.insert("active".to_string(), PropertyValue::Bool(index % 2 == 0));
            properties.insert(
                "classification".to_string(),
                PropertyValue::Str(format!("c{}", index % 5)),
            );

            Rule {
                id: format!("rule-{index:06}"),
                properties,
                geometry: geo::Geometry::MultiPolygon(MultiPolygon::new(vec![Polygon::new(
                    ring(&mut rng, cx, cy, scale.vertices),
                    vec![],
                )])),
            }
        })
        .collect()
}

/// Generate `count` small square candidates scattered over the same grid
/// extent the rules occupy, deterministically.
pub fn generate_candidates(count: usize, scale: Scale) -> Vec<Candidate> {
    let mut rng = crate::dataset::Rng::new(0xCAFE_2026_F00D);
    let columns = (scale.rules as f64).sqrt().ceil() as usize;
    let extent = columns.max(1) as f64 * 10.0;
    (0..count)
        .map(|index| {
            let cx = rng.f64() * extent - extent / 2.0;
            let cy = rng.f64() * extent - extent / 2.0;
            let half = 0.25;
            let corners = [
                (cx - half, cy - half),
                (cx - half, cy + half),
                (cx + half, cy + half),
                (cx + half, cy - half),
                (cx - half, cy - half),
            ];
            let line = LineString::from(
                corners
                    .iter()
                    .map(|(x, y)| Coord { x: *x, y: *y })
                    .collect::<Vec<_>>(),
            );
            Candidate::new(
                format!("candidate-{index:06}"),
                geo::Geometry::Polygon(Polygon::new(line, vec![])),
            )
        })
        .collect()
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
