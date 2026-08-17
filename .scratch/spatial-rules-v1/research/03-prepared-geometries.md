# 03 — Prepared geometries in geo: primary-source research (2026-08-14)

Supports the Prepared-geometry options ticket. Sources: docs.rs (0.33.1), GitHub georust/geo source + CHANGELOG + PRs.

## `PreparedGeometry` (geo 0.33.1)

- Wraps one geometry + cached `GeometryGraph` (self-noded topology) + R*-tree edge index + bounding rect. Bounds `G: Into<GeometryCow<'a,F>>, F: GeoFloat + RTreeNum`. https://github.com/georust/geo/blob/main/geo/src/algorithm/indexed/prepared_geometry.rs
- Doc: "backed by an R*-tree spatial index and can be more efficient than a plain Geometry when performing multiple topological comparisons." https://docs.rs/geo/0.33.1/geo/algorithm/indexed/prepared_geometry/struct.PreparedGeometry.html
- Accelerates **only `Relate`** — the geometry graph is cached, so `relate()` skips re-noding/re-indexing. You get intersects/contains/within via `prepared.relate(&other).is_intersects()/is_contains()/is_within()`. No direct `Contains`/`Intersects` impls.
- Construction cost: `GeometryGraph::new(…)` — full self-noding + R-tree at construction. A large landmask MultiPolygon measured ~4.4 s (vs ~0.25 s WKB parse). https://github.com/georust/geo/pull/1571
- **`!Send` and `!Sync`** in 0.33.1 (auto-trait section). Cause: `GeometryGraph` holds `Rc<RefCell<…>>`. https://github.com/georust/geo/pull/1197
- **`main` (unreleased): `PreparedGeometry` is now `Send`** (CHANGELOG Unreleased; PR #1571, merged ~2026-08-13) but **still not `Sync`** (contains `RefCell` internally; `unsafe impl Sync` deemed unsound). Ships in next release 0.34.x.
- Implication for `Arc<Ruleset>`: in 0.33.1 you cannot store `PreparedGeometry` in an `Arc` shared with async workers (`!Send` blocks moving, `!Sync` blocks `&` sharing); on `main` it's movable but still not shareable — per-worker ownership or `Mutex`.

## `MonotoneChainPolygon` / `MonotoneChainMultiPolygon` (0.33.0+)

- Added 0.33.0: "Preprocessing cost… significant performance boost for **intersects and contains_properly** checks." https://github.com/georust/geo/blob/main/geo/CHANGES.md
- Impls (0.33.1): `Intersects` (MC↔MC), `ContainsProperly` (MC polygon↔MC multipolygon), `BoundingRect`, `HasDimensions`, `From<&Polygon>`/`From<&MultiPolygon>`. **No `Relate`, no plain `Contains`, no `Within`.**
- Send/Sync: yes (with `f64`). Borrows the source polygon (`'a`).
- Both sides must be monotone for the fast path — `MCPoly.intersects(plain_poly)` delegates to the slow path. No one-sided property (unlike `PreparedGeometry`). https://github.com/georust/geo/pull/1467

## `IntervalTreeMultiPolygon` (0.32.0+)

- "A MultiPolygon backed by an interval tree for fast containment queries." Accelerates only `Contains<Point/Coord>` (0.33.1); unreleased adds `Intersects<Point/Coord>`. **No polygon-vs-polygon help.** Send/Sync: yes.
- Construction 4–5× faster than `PreparedGeometry`; point containment ~2–3 µs. https://github.com/georust/geo/pull/1571

## Plain `Relate`

- Each call builds a **fresh** noded topology graph + R-tree edge index for both sides, then computes the DE-9IM matrix — the cached part in `PreparedGeometry` is exactly this. https://docs.rs/geo/0.33.1/src/geo/algorithm/relate/mod.rs.html

## Expected win (30 rules × ~1,000 candidates)

- Published number: prepared `relate` ≈ 49.0 ms vs unprepared ≈ 842.9 ms (**~17×**) for repeated polygon relates. https://github.com/georust/geo/pull/1197
- 30,000 relates/request ⇒ prepared-vs-unprepared is the likely dominant factor. The `Send` fix costs +2.4% (JTS relate) / +7.8% (disjoint) on some suites. https://github.com/georust/geo/pull/1571

## `Send` fix status

- No released geo has a `Send` `PreparedGeometry`; latest release 0.33.1 (2026-04-20) is `!Send`. Fix merged to `main` (PR #1571), listed Unreleased → next release 0.34.x.

## Options (concurrently-shared ruleset)

- **A. Per-worker `PreparedGeometry` (0.33.1).** Store plain polygons in `Arc<Ruleset>`; each worker builds/clones its 30 `PreparedGeometry` inside the thread. `!Send` blocks crossing threads. Best per-query speed.
- **B. Pin geo to git `main`.** Prebuild 30 once, `clone()` per worker, move in. `Send` yes / `Sync` no → per-worker ownership or `Mutex`. Unstable dependency.
- **C. `MonotoneChain*`.** Send+Sync, but only `intersects` + `contains_properly`, both-sides-MC.
- **D. `IntervalTreeMultiPolygon`.** Send+Sync, point containment only — not applicable.
- **E. Skip preparation; plain `Relate`.** Send+Sync, correct, ~17× slower per relate.

**Recommendation:** on released 0.33.1 use **A** — store plain polygons in the shared `Arc`, prepare per worker inside the thread, and relate one-sided `prepared_rule.relate(&candidate)` (answer `within` as `candidate.relate(&prepared_rule).is_within()` so the rule stays prepared). Revisit at geo 0.34 (`Send` fix). The final prepared-vs-unprepared adoption is decided by the harness task's ladder E/F (§32).
