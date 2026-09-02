//! Report schemas and measurement config for the memory-scaling benchmark.
//!
//! These types describe one scale cell: the grid position (rule count ×
//! vertices), the per-cell knobs, and the machine-readable report every cell
//! emits in its own child process. They are pure data — no measurement logic.

use serde::{Deserialize, Serialize};

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
