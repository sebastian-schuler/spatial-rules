# 11 — turf.js predicate semantics vs DE-9IM: research (2026-08-17)

Supports the Predicate semantics ticket. ⚠️ Web fetch was unavailable this session; findings come from stable, high-confidence knowledge of turf/JSTS behavior, with canonical primary-source URLs to verify. Facts marked **[V]** should be re-checked live before final reliance.

## `@turf/boolean-intersects`

- v6 impl: `!booleanDisjoint(f1, f2)`; `boolean-disjoint` via `jsts.operation.relate.RelateOp.relate(g1,g2).isDisjoint()`. https://github.com/Turfjs/turf/blob/v6.5.0/packages/turf-boolean-intersects/index.js
- Result: intersects = true iff matrix ≠ `FF*FF****` — **touching polygons → true**, matching DE-9IM. https://turfjs.org/docs/api/booleanIntersects
- Miss mode is floating-point, not semantics: turf does not snap, so near-coincident coordinates (1e-15 gaps) give false `false`.

## `@turf/boolean-contains` / `@turf/boolean-within`

- v6: `boolean-contains` = `booleanWithin(f2, f1)`; `boolean-within` uses JSTS `Geometry.within` → `RelateOp.isWithin/isContains` → DE-9IM `T*F**F***` (contains) / `T**F*F***` (within). Interior-to-interior required — touching from outside is NOT contained. https://github.com/Turfjs/turf/blob/v6.5.0/packages/turf-boolean-within/index.js
- Holes handled per JTS: a polygon inside another's hole is not within/contained (hole interior = outer's exterior).
- **Known quirk:** point-in-polygon does NOT use JTS — ray-casting `@turf/boolean-point-in-polygon` counts boundary points as inside (`ignoreBoundary:false`), so `booleanContains(polygon, pointOnBoundary)` = `true` vs DE-9IM `false`. Only relevant for point inputs (not in our polygon/multipolygon matrix). https://turfjs.org/docs/api/booleanPointInPolygon

## JSTS relationship (version-dependent)

- turf **v6** depends on `jsts` (v2.x), a line-for-line JTS port; `Geometry.intersects/contains/within` delegate to `RelateOp`. For polygon-polygon with boundary contact, **JSTS = JTS/GEOS/DE-9IM exactly**. https://github.com/bjornharrtell/jsts
- **turf v7 (2024) removed `jsts`** and rewrote the `boolean-*` packages natively in TS; fidelity is implementation-defined, with reported regressions. **[V]** — pin the turf major deliberately.

## Known discrepancies

- Point-on-boundary (contains/within): turf `true` vs JTS/GEOS `false` (documented; point-only).
- Float precision / no snapping: "touches but intersects=false" for near-coincident coords; recurring issue class, not semantic. **[H]**
- v7 native-rewrite regressions (identical polygons, holes, MultiPolygon shared edges, degenerate input) reported after 7.0.0. **[V]** — verify at https://github.com/Turfjs/turf/issues?q=is%3Aissue+boolean
- Invalid/degenerate input: no validation; undefined results; JTS may throw `TopologyException` ("Invalid ring"). turf READMEs advise pre-cleaning (`@turf/clean-coords` / `truncate` / `polygon-clipping`).
- Identical geometries: DE-9IM-consistent — contains/within/intersects all `true`, disjoint `false`.

## Accepted types & invalid handling

- v6 boolean-* docs: Point/LineString/Polygon/MultiPolygon Features or Geometry objects; **FeatureCollection/GeometryCollection not supported** (throws) **[V]**; Polygon/MultiPolygon fully supported incl. holes; no validity gate — garbage-in/garbage-out.

## Reconciliation (for the test matrix)

1. **Authority:** geo `Relate` DE-9IM is authoritative; turf is an independent oracle, not the spec. A disagreement is a flag to investigate, not a reason to flip geo's answer.
2. **Known divergences to expect:** point-on-boundary (exclude — not our geometry types); near-coincident touching (normalize both sides to a shared precision before cross-checking).
3. **Pin the turf major deliberately** in devDependencies: v6 = JTS-faithful (best oracle); v7 = suspect secondary oracle — confirm any disagreement against v6/JSTS/GEOS.
4. **Special-case in the matrix:** identical geometries (all three `true`); polygon-in-hole (`contains`/`within` `false`, both agree); touching-at-edge (`intersects` `true`); overlapping boundaries; degenerate/zero-area input — **skip cross-check** (undefined on both sides; validate first per ADR-0005).
5. **Escalation:** any remaining disagreement → minimal GeoJSON fixture, confirm against JSTS/GEOS, record as a known-quirk entry with the turf issue link; if a v7 regression, prefer the v6/JSTS result.
