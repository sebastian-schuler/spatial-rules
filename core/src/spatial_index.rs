//! Spatial index over precomputed rule envelopes (ADR-0002).
//!
//! The default index is a packed `rstar` R*-tree (bulk-loaded); a linear
//! envelope scan is retained as the benchmark-ladder baseline. Both sit
//! behind the [`SpatialIndex`] trait so the ladder can swap them.

use geo::Rect;
use rstar::primitives::GeomWithData;
use rstar::{AABB, RTree, RTreeObject};

use crate::rule::RuleId;

/// Answers envelope-intersection queries against indexed rule envelopes.
pub trait SpatialIndex: Send + Sync {
    /// Append rule ids whose envelope intersects `envelope` into `out`, sorted
    /// ascending and deduplicated (architecture-hardening 03). The caller owns
    /// and reuses `out` across a batch, so the per-candidate allocation moves
    /// out of the query hot loop.
    fn query_envelope_into(&self, envelope: &Rect<f64>, out: &mut Vec<RuleId>);

    /// Rule ids whose envelope intersects `envelope`, sorted ascending.
    fn query_envelope(&self, envelope: &Rect<f64>) -> Vec<RuleId> {
        let mut out = Vec::new();
        self.query_envelope_into(envelope, &mut out);
        out
    }
}

/// The selectable index implementations (benchmark ladder, ADR-0002).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SpatialIndexKind {
    /// Packed `rstar` R*-tree (`bulk_load`) — the default.
    RStar,
    /// Linear envelope scan — retained as the ladder baseline.
    LinearScan,
}

/// A rule envelope as an [`RTreeObject`]; the id rides alongside in
/// [`GeomWithData`].
#[derive(Debug, Clone, Copy)]
struct RuleEnvelope {
    rect: Rect<f64>,
}

impl RTreeObject for RuleEnvelope {
    type Envelope = AABB<[f64; 2]>;

    fn envelope(&self) -> Self::Envelope {
        rect_to_aabb(&self.rect)
    }
}

fn rect_to_aabb(rect: &Rect<f64>) -> AABB<[f64; 2]> {
    AABB::from_corners(
        [(*rect).min().x, (*rect).min().y],
        [(*rect).max().x, (*rect).max().y],
    )
}

/// Packed `rstar` R*-tree index (default, ADR-0002).
pub struct RStarIndex {
    tree: RTree<GeomWithData<RuleEnvelope, RuleId>>,
}

impl RStarIndex {
    pub fn build(entries: Vec<(Rect<f64>, RuleId)>) -> Self {
        let items = entries
            .into_iter()
            .map(|(rect, id)| GeomWithData::new(RuleEnvelope { rect }, id))
            .collect();
        RStarIndex {
            tree: RTree::bulk_load(items),
        }
    }
}

impl SpatialIndex for RStarIndex {
    fn query_envelope_into(&self, envelope: &Rect<f64>, out: &mut Vec<RuleId>) {
        let aabb = rect_to_aabb(envelope);
        out.clear();
        out.extend(
            self.tree
                .locate_in_envelope_intersecting(aabb)
                .map(|entry| entry.data),
        );
        out.sort_unstable();
        out.dedup();
    }
}

/// Linear envelope scan (benchmark-ladder baseline, ADR-0002).
pub struct LinearScanIndex {
    entries: Vec<(Rect<f64>, RuleId)>,
}

impl LinearScanIndex {
    pub fn build(entries: Vec<(Rect<f64>, RuleId)>) -> Self {
        LinearScanIndex { entries }
    }
}

impl SpatialIndex for LinearScanIndex {
    fn query_envelope_into(&self, envelope: &Rect<f64>, out: &mut Vec<RuleId>) {
        out.clear();
        out.extend(
            self.entries
                .iter()
                .filter(|(rect, _)| rects_intersect(rect, envelope))
                .map(|(_, id)| *id),
        );
        out.sort_unstable();
        out.dedup();
    }
}

fn rects_intersect(a: &Rect<f64>, b: &Rect<f64>) -> bool {
    (*a).min().x <= (*b).max().x
        && (*a).max().x >= (*b).min().x
        && (*a).min().y <= (*b).max().y
        && (*a).max().y >= (*b).min().y
}

/// Build the spatial index for a given kind from `(envelope, rule_id)` entries.
pub fn build_spatial_index(
    kind: SpatialIndexKind,
    entries: Vec<(Rect<f64>, RuleId)>,
) -> Box<dyn SpatialIndex> {
    match kind {
        SpatialIndexKind::RStar => Box::new(RStarIndex::build(entries)),
        SpatialIndexKind::LinearScan => Box::new(LinearScanIndex::build(entries)),
    }
}
