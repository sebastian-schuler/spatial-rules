//! Thread-safe ruleset holder with atomic replacement (ADR-0007, ADR-0009).

use std::sync::{Arc, Mutex, RwLock};
use std::time::{Instant, SystemTime, UNIX_EPOCH};

use crate::candidate::Candidate;
use crate::error::SpatialError;
use crate::query::{CandidateOutcome, Query};
use crate::rule::Rule;
use crate::ruleset::Ruleset;

/// Observability for the active ruleset, returned by `replace()` (ADR-0007).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ReplaceReport {
    /// Monotonic active-ruleset id (the initial ruleset is `1`).
    pub version: u64,
    pub rule_count: usize,
    pub build_duration_ms: u64,
    pub last_swap_time_unix_ms: u64,
}

fn now_unix_ms() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|duration| duration.as_millis() as u64)
        .unwrap_or(0)
}

/// Owns the active ruleset and swaps it atomically on `replace`.
///
/// A query snapshots the current `Arc<Ruleset>` under a read lock and releases
/// the lock immediately, so a replacement never blocks a query and the old
/// ruleset stays alive until the last in-flight query drops its snapshot
/// (ADR-0007). The swap itself is a single `Arc` write under the write lock —
/// readers see either the old or the new ruleset, never a partial build.
pub struct Engine {
    ruleset: RwLock<Arc<Ruleset>>,
    state: Mutex<ReplaceReport>,
}

impl Engine {
    /// Wrap a compiled ruleset as the initial active ruleset.
    fn wrap(ruleset: Ruleset) -> Result<Self, SpatialError> {
        let rule_count = ruleset.len();
        Ok(Engine {
            ruleset: RwLock::new(Arc::new(ruleset)),
            state: Mutex::new(ReplaceReport {
                version: 1,
                rule_count,
                build_duration_ms: 0,
                last_swap_time_unix_ms: now_unix_ms(),
            }),
        })
    }

    /// Build an engine from an already-parsed rule list.
    pub fn new(rules: Vec<Rule>) -> Result<Self, SpatialError> {
        Self::wrap(Ruleset::build(rules)?)
    }

    /// Build an engine from a GeoJSON FeatureCollection.
    pub fn from_geojson(input: &str) -> Result<Self, SpatialError> {
        Self::wrap(Ruleset::from_geojson(input)?)
    }

    /// Evaluate a batch against the current ruleset (same semantics as
    /// [`Ruleset::query`]).
    pub fn query(&self, candidates: &[Candidate], query: &Query) -> Vec<CandidateOutcome> {
        self.snapshot().query(candidates, query)
    }

    /// Evaluate a batch and return the compact `0/1/2` mask (ADR-0004),
    /// without materialising per-match rule ids.
    pub fn query_mask(&self, candidates: &[Candidate], query: &Query) -> Vec<u8> {
        self.snapshot().query_mask(candidates, query)
    }

    /// A cheap snapshot of the current ruleset; kept alive by the caller.
    pub fn snapshot(&self) -> Arc<Ruleset> {
        self.ruleset
            .read()
            .unwrap_or_else(|poisoned| poisoned.into_inner())
            .clone()
    }

    /// Build a new ruleset fully off the hot path, then publish it atomically.
    /// Returns the ADR-0007 observability (ADR-0009).
    pub fn replace(&self, rules: Vec<Rule>) -> Result<ReplaceReport, SpatialError> {
        let started = Instant::now();
        let new_ruleset = Ruleset::build(rules)?;
        Ok(self.swap(new_ruleset, started.elapsed().as_millis() as u64))
    }

    /// Replace from a GeoJSON FeatureCollection.
    pub fn replace_from_geojson(&self, input: &str) -> Result<ReplaceReport, SpatialError> {
        let started = Instant::now();
        let new_ruleset = Ruleset::from_geojson(input)?;
        Ok(self.swap(new_ruleset, started.elapsed().as_millis() as u64))
    }

    /// Replace from canonical ruleset JSON (ADR-0013): load off the hot path,
    /// then publish atomically so a failed load keeps the old ruleset.
    pub fn replace_from_canonical(&self, input: &[u8]) -> Result<ReplaceReport, SpatialError> {
        let started = Instant::now();
        let new_ruleset = Ruleset::from_canonical(input)?;
        Ok(self.swap(new_ruleset, started.elapsed().as_millis() as u64))
    }

    /// Publish a compiled ruleset and update observability in one critical
    /// section, so a `current()` read never observes the new ruleset with
    /// stale counters. No other path holds both locks, so the ordering is
    /// deadlock-free.
    fn swap(&self, new_ruleset: Ruleset, build_duration_ms: u64) -> ReplaceReport {
        let rule_count = new_ruleset.len();
        let last_swap_time_unix_ms = now_unix_ms();

        let mut ruleset = self
            .ruleset
            .write()
            .unwrap_or_else(|poisoned| poisoned.into_inner());
        let mut state = self
            .state
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner());
        state.version += 1;
        state.rule_count = rule_count;
        state.build_duration_ms = build_duration_ms;
        state.last_swap_time_unix_ms = last_swap_time_unix_ms;
        *ruleset = Arc::new(new_ruleset);
        *state
    }

    /// Observability for the current ruleset.
    pub fn current(&self) -> ReplaceReport {
        *self
            .state
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner())
    }
}
