//! Memory-boundedness heuristics for the memory-scaling benchmark.
//!
//! These are pure functions over observation series — no process state is
//! touched. They answer two questions a leak check needs: whether two
//! observations are within tolerance of each other, and whether a *series* of
//! per-cycle observations shows evidence of a per-cycle leak. Unit-tested in
//! the parent module.

/// A relative tolerance for the boundedness classification: resident memory
/// may legitimately wobble by this fraction of the steady-state footprint
/// (allocator retention, page trimming) without indicating a leak.
pub const BOUNDED_TOLERANCE_RATIO: f64 = 0.05;

/// The tail-plateau tolerance for [`is_bounded_series`]: the last quarter is
/// "flat" only when its range is within this fraction of its mean. Deliberately
/// **tighter** than [`BOUNDED_TOLERANCE_RATIO`] (memory-benchmark ticket 03):
/// the plateau signal exists to recognize a warmup that has *settled* (whose
/// flat tail has ~0% range on Linux), and a loose tolerance would instead
/// rescue a slow leak — a 5-sample tail spans 4 per-swap intervals, so a 2%
/// plateau still rejects any leak above ~0.5% of the footprint per swap,
/// matching the steady test's detection boundary.
pub const PLATEAU_RANGE_RATIO: f64 = 0.02;

/// [`is_bounded_series`] needs at least this many samples (16 = four
/// 4-point quarters) before the reset/plateau signals are meaningful; below
/// it only the steady test applies.
pub const MIN_RESET_PLATEAU_SAMPLES: usize = 16;

/// Whether resident memory is *bounded* across a repeated operation: the
/// spread between the last and first observation must sit within
/// [`BOUNDED_TOLERANCE_RATIO`] of the **first** observation. A leak makes
/// `last` climb monotonically past any such tolerance.
pub fn is_bounded(first_bytes: u64, last_bytes: u64) -> bool {
    let spread = last_bytes.abs_diff(first_bytes) as f64;
    spread <= BOUNDED_TOLERANCE_RATIO * first_bytes as f64
}

/// Whether a *series* of observations is bounded — i.e. shows no evidence of a
/// per-cycle leak. A plain 5%-tolerance comparison of the second vs fourth
/// quarter means (the original heuristic) is a false positive on two bounded
/// shapes a short window can produce (memory-benchmark ticket 03):
///
/// - a **one-time warmup step** that then runs flat (allocator arenas filling
///   on the first swaps), and
/// - a **bounded sawtooth** (glibc growing the arena ~21 MiB per replacement
///   and trimming it back to the same floor every ~11–18 swaps) — the window
///   can land on an up-slope and read as drift.
///
/// A series is therefore bounded when *any* of these hold, each only claiming
/// what the window supports:
///
/// - **steady**: `|mean(q4) − mean(q2)|` ≤ tolerance (the original test);
/// - **reset**: some sample of the last quarter revisits within tolerance of
///   the second quarter's floor — a sawtooth that completed a trim inside the
///   window (a monotone leak never revisits its earlier floor, so this is
///   leak-proof);
/// - **tail plateau**: the last quarter is flat (range ≤ [`PLATEAU_RANGE_RATIO`]
///   of its mean) — warmup that has settled. The plateau tolerance is tight
///   enough that a slow leak's still-climbing tail is not mistaken for flat.
///
/// Otherwise the verdict is `false` = **not proven bounded**: the window shows
/// drift it cannot explain — either an up-slope with no reset yet (e.g. a
/// 20-swap window landing between two glibc trims, whose phase varies run to
/// run) or a genuine leak. The two are indistinguishable within one window; a
/// 50-swap probe spanning several trim cycles distinguishes them. The
/// reset/plateau signals need [`MIN_RESET_PLATEAU_SAMPLES`] samples
/// (4-point quarters) to be meaningful; below that only the steady test
/// applies. Returns `None` with fewer than 4 observations — no claim.
pub fn is_bounded_series(samples: &[u64]) -> Option<bool> {
    if samples.len() < 4 {
        return None;
    }
    let quarter = samples.len() / 4;
    let mean = |slice: &[u64]| -> f64 {
        slice.iter().map(|&value| value as f64).sum::<f64>() / slice.len() as f64
    };
    let q2 = mean(&samples[quarter..2 * quarter]);
    let q4 = mean(&samples[3 * quarter..]);
    let steady = (q4 - q2).abs() <= BOUNDED_TOLERANCE_RATIO * q2.max(1.0);
    if steady {
        return Some(true);
    }
    if samples.len() < MIN_RESET_PLATEAU_SAMPLES {
        // Too few samples to tell a bounded shape from drift.
        return Some(false);
    }
    let min_of = |start: usize, end: usize| samples[start..end].iter().copied().min().unwrap();
    let reset = (min_of(3 * quarter, samples.len()) as f64)
        <= (1.0 + BOUNDED_TOLERANCE_RATIO) * (min_of(quarter, 2 * quarter) as f64).max(1.0);
    let tail = &samples[3 * quarter..];
    let tail_min = *tail.iter().min().unwrap();
    let tail_max = *tail.iter().max().unwrap();
    let tail_plateau = (tail_max - tail_min) as f64 <= PLATEAU_RANGE_RATIO * mean(tail).max(1.0);
    Some(reset || tail_plateau)
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
