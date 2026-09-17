//! Great-circle (spherical) **fringe** box for a rule geometry.
//!
//! The `withinDistance` pre-filter asks the spatial index for the rule boxes
//! near the candidate, then confirms exactly with a haversine closest-point
//! search. But the planar box is *not* conservative for that confirm: it walks
//! rule edges as **great-circle arcs**, which bulge outside the endpoints'
//! planar box (at high latitude, and for long diagonal edges anywhere). A purely
//! planar index therefore drops rules that are genuinely within N metres
//! (`.scratch/perf-memory/issues/09`).
//!
//! The fix is one **fringe** box per rule: the bounding box of the arcs, emitted
//! only when it escapes the planar box. `withinDistance` queries the planar
//! index *and* the fringe index and unions the hits; every other predicate uses
//! the planar index alone (it compares planar geometries, where the planar box
//! is already exact).
//!
//! Deliberately **one box per rule**, not one per edge (ticket 11): an
//! edge-granular fringe gave better selectivity but grew with *vertices*, and at
//! 100k rules it doubled the ruleset footprint (measured: 816 → 1626 B/rule,
//! 77.8 → 155.1 MiB). One box per rule is bounded by rule count and costs
//! ~32 B/rule. The residual cost is pre-filter selectivity on the distance
//! surface, which is the price of a box filter over arc geometry.
//!
//! Sampling is at [`SAMPLE_STEP_DEGREES`] with [`MARGIN_DEGREES`] added to cover
//! the residual sagitta. Over-approximation is deliberate (a wide border, an
//! antimeridian-crossing edge): it costs pre-filter selectivity, never
//! correctness.

use geo::{Coord, Geometry, LineString, Rect};

/// Sub-arc length used when sampling an edge's great circle, in degrees.
/// 0.1° gives a worst-case sagitta of ~2.4 m on a mean-radius sphere.
const SAMPLE_STEP_DEGREES: f64 = 0.1;

/// Margin added to the fringe box, in degrees (~11 m): comfortably covers the
/// ~2.4 m sampling sagitta at [`SAMPLE_STEP_DEGREES`].
const MARGIN_DEGREES: f64 = 0.0001;

/// The **fringe** box of `geometry` beyond its planar box `planar`, or `None`
/// when every arc stays inside it (true at low latitude, where the bulge is
/// below the sampling resolution).
///
/// Rule geometries are validated to Polygon/MultiPolygon; any other variant
/// produces no fringe (and is rejected by the build gate before it can become a
/// rule anyway).
pub(crate) fn fringe_box(geometry: &Geometry<f64>, planar: &Rect<f64>) -> Option<Rect<f64>> {
    let mut bounds = Bounds::empty();
    match geometry {
        Geometry::Polygon(polygon) => add_polygon_arcs(polygon, &mut bounds),
        Geometry::MultiPolygon(multi) => {
            for polygon in &multi.0 {
                add_polygon_arcs(polygon, &mut bounds);
            }
        }
        _ => return None,
    }
    if contains_rect(planar, &bounds.rect(0.0)?) {
        return None;
    }
    bounds.rect(MARGIN_DEGREES)
}

fn add_polygon_arcs(polygon: &geo::Polygon<f64>, bounds: &mut Bounds) {
    add_ring_arcs(polygon.exterior(), bounds);
    for hole in polygon.interiors() {
        add_ring_arcs(hole, bounds);
    }
}

fn add_ring_arcs(ring: &LineString<f64>, bounds: &mut Bounds) {
    for line in ring.lines() {
        bounds.add_arc(line.start, line.end);
    }
}

/// Whether `inner` lies entirely within `outer`.
fn contains_rect(outer: &Rect<f64>, inner: &Rect<f64>) -> bool {
    inner.min().x >= outer.min().x
        && inner.max().x <= outer.max().x
        && inner.min().y >= outer.min().y
        && inner.max().y <= outer.max().y
}

/// A running min/max over sampled longitude/latitude pairs.
#[derive(Clone, Copy)]
struct Bounds {
    min_x: f64,
    min_y: f64,
    max_x: f64,
    max_y: f64,
    empty: bool,
}

impl Bounds {
    fn empty() -> Self {
        Self {
            min_x: f64::INFINITY,
            min_y: f64::INFINITY,
            max_x: f64::NEG_INFINITY,
            max_y: f64::NEG_INFINITY,
            empty: true,
        }
    }

    fn add(&mut self, x: f64, y: f64) {
        self.min_x = self.min_x.min(x);
        self.min_y = self.min_y.min(y);
        self.max_x = self.max_x.max(x);
        self.max_y = self.max_y.max(y);
        self.empty = false;
    }

    /// Add an edge and samples of the great-circle arc between its endpoints.
    fn add_arc(&mut self, start: Coord<f64>, end: Coord<f64>) {
        self.add(start.x, start.y);
        self.add(end.x, end.y);

        let omega = central_angle(start, end);
        // Coincident endpoints need no sampling; a (pathological) antipodal edge
        // has no unique arc, so we keep the endpoint box rather than a degenerate
        // slerp — the same coverage the planar box gave.
        if !(1e-12..std::f64::consts::PI - 1e-12).contains(&omega) {
            return;
        }
        let steps = (omega.to_degrees() / SAMPLE_STEP_DEGREES).ceil() as usize;
        for index in 1..steps {
            let ratio = index as f64 / steps as f64;
            if let Some((lon, lat)) = interpolate(start, end, omega, ratio) {
                self.add(lon, lat);
            }
        }
    }

    /// The accumulated box expanded by `margin`, or `None` when nothing was
    /// added.
    fn rect(self, margin: f64) -> Option<Rect<f64>> {
        if self.empty {
            return None;
        }
        Some(Rect::new(
            (self.min_x - margin, self.min_y - margin),
            (self.max_x + margin, self.max_y + margin),
        ))
    }
}

/// Angle, in radians, between two longitude/latitude coordinates on a unit
/// sphere.
fn central_angle(a: Coord<f64>, b: Coord<f64>) -> f64 {
    let a = to_unit(a);
    let b = to_unit(b);
    let dot = (a.0 * b.0 + a.1 * b.1 + a.2 * b.2).clamp(-1.0, 1.0);
    dot.acos()
}

/// The point at `ratio` along the great-circle arc from `a` to `b` (spherical
/// linear interpolation), given the arc's [`central_angle`]. Returns `None`
/// when the arc is degenerate.
fn interpolate(a: Coord<f64>, b: Coord<f64>, omega: f64, ratio: f64) -> Option<(f64, f64)> {
    let sin_omega = omega.sin();
    if sin_omega.abs() < 1e-12 {
        return None;
    }
    let a = to_unit(a);
    let b = to_unit(b);
    let wa = ((1.0 - ratio) * omega).sin() / sin_omega;
    let wb = (ratio * omega).sin() / sin_omega;
    let x = wa * a.0 + wb * b.0;
    let y = wa * a.1 + wb * b.1;
    let z = wa * a.2 + wb * b.2;
    let lon = y.atan2(x).to_degrees();
    let lat = z.atan2((x * x + y * y).sqrt()).to_degrees();
    Some((lon, lat))
}

/// Longitude/latitude to a unit vector, `(x, y, z)`.
fn to_unit(coord: Coord<f64>) -> (f64, f64, f64) {
    let lon = coord.x.to_radians();
    let lat = coord.y.to_radians();
    let (sin_lat, cos_lat) = lat.sin_cos();
    let (sin_lon, cos_lon) = lon.sin_cos();
    (cos_lat * cos_lon, cos_lat * sin_lon, sin_lat)
}

#[cfg(test)]
mod tests {
    use super::*;
    use geo::{BoundingRect, Polygon};

    fn rect_geometry(min_x: f64, min_y: f64, max_x: f64, max_y: f64) -> Geometry<f64> {
        let polygon: Polygon<f64> = Rect::new((min_x, min_y), (max_x, max_y)).into();
        Geometry::Polygon(polygon)
    }

    /// A rule contributes at most one fringe box — the memory bound.
    #[test]
    fn at_most_one_fringe_box() {
        let geometry = rect_geometry(0.0, 0.0, 10.0, 10.0);
        let planar = geometry.bounding_rect().unwrap();
        // `Option` proves it structurally; this asserts it is populated here.
        assert!(fringe_box(&geometry, &planar).is_some());
    }

    /// A tiny polygon at the equator has no meaningful bulge, so it pays
    /// nothing.
    #[test]
    fn tiny_equatorial_polygon_has_no_fringe() {
        let geometry = rect_geometry(0.0, 0.0, 0.001, 0.001);
        let planar = geometry.bounding_rect().unwrap();
        assert!(fringe_box(&geometry, &planar).is_none());
    }

    #[test]
    fn equator_rule_barely_grows() {
        let geometry = rect_geometry(0.0, 0.0, 10.0, 10.0);
        let planar = geometry.bounding_rect().unwrap();
        let fringe = fringe_box(&geometry, &planar).unwrap();
        // The bulge at 10°N is tiny; the margin dominates.
        assert!((fringe.max().y - planar.max().y) < 0.05);
    }

    /// The ticket-09 counterexample: the rect's top edge sits at 81.6138°N but
    /// its great-circle arc reaches ~81.625°N, ~1.2 km further poleward than the
    /// planar box.
    #[test]
    fn high_latitude_fringe_reaches_the_arc() {
        let geometry = rect_geometry(
            72.30009202046368,
            71.72880835080687,
            80.17047556605961,
            81.61376390875111,
        );
        let planar = geometry.bounding_rect().unwrap();
        let fringe = fringe_box(&geometry, &planar).unwrap();

        assert!(
            fringe.max().y > planar.max().y + 0.005,
            "expected a poleward fringe well beyond the ~11 m margin: planar {} vs fringe {}",
            planar.max().y,
            fringe.max().y
        );
        // The arc's maximum latitude is atan(tan(81.6138°)/cos(3.935°)) ≈ 81.6252°.
        assert!((fringe.max().y - 81.6252).abs() < 0.01);
    }

    /// The covered region must contain the arcs, not just reach past them.
    #[test]
    fn fringe_contains_the_arc_midpoint() {
        let geometry = rect_geometry(0.0, 60.0, 40.0, 60.0);
        let planar = geometry.bounding_rect().unwrap();
        let start = Coord { x: 0.0, y: 60.0 };
        let end = Coord { x: 40.0, y: 60.0 };
        let mid = interpolate(start, end, central_angle(start, end), 0.5).unwrap();
        assert!(mid.1 > 60.0, "the arc should bulge north of the edge");

        let fringe = fringe_box(&geometry, &planar).unwrap();
        assert!(
            fringe.min().x <= mid.0
                && fringe.max().x >= mid.0
                && fringe.min().y <= mid.1
                && fringe.max().y >= mid.1,
            "the fringe box must cover the arc midpoint {mid:?}"
        );
    }
}
