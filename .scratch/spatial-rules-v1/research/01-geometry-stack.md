# 01 — Geometry stack: primary-source research (2026-08-13)

Supports the Geometry stack ticket. All claims cite primary sources (crates.io, docs.rs, GitHub).

## `geo` (georust)

- **0.33.1**, released 2026-04-20, 20.69M downloads, MIT OR Apache-2.0. https://crates.io/crates/geo
- Types: `Polygon` (exterior + interior rings) and `MultiPolygon` re-exported from `geo-types`, OGC Simple Features. https://docs.rs/geo
- **Coordinates are planar/euclidean** ("The geo crate provides planar geospatial geometries and algorithms"). https://docs.rs/geo
- `Intersects` is DE-9IM-defined (`FF*FF****` not satisfied), symmetric; bbox check is a fast-path only, not false-positive-generating. https://docs.rs/geo/latest/geo/algorithm/intersects/trait.Intersects.html
- `Contains` = DE-9IM `T*F**F***`; `Within` = `Contains` with arguments swapped. https://docs.rs/geo
- `Relate` gives full DE-9IM `IntersectionMatrix` (`.is_intersects()/.is_contains()/.is_within()/.is_touches()`). https://docs.rs/geo
- Correctness history: "Use robust predicates everywhere" (0.22.0), `Contains` rewritten via `Relate` (0.18.0), removed unreliable polygon-polygon fast path (0.23.1). https://github.com/georust/geo/blob/main/geo/CHANGES.md
- `Validation` trait: `is_valid()`, `validation_errors()`, `check_validation()`. https://docs.rs/geo
- `PreparedGeometry` (R*-tree backed) exists at crate root; implements `Relate`; in released 0.33.1 it is `!Send`/`!Sync` (`Send` arrives in unreleased changelog #1571). https://docs.rs/geo/latest/geo/algorithm/indexed/prepared_geometry/struct.PreparedGeometry.html
- Also `MonotoneChainPolygon/MultiPolygon` and `IntervalTreeMultiPolygon` precomputed structures. https://docs.rs/geo

## `geo-types` + `geojson`

- `geo-types` 0.7.20, MIT OR Apache-2.0, 24.58M downloads. https://crates.io/crates/geo-types
- `geojson` 1.0.0, MIT OR Apache-2.0, 10.33M downloads. https://crates.io/crates/geojson
- Parsing: default-on `geo-types` feature gives fallible `TryFrom` → `geo_types::Geometry<f64>` from `FeatureCollection`/`Feature`/`GeometryValue`; `MultiPolygon` represented. https://docs.rs/geojson
- Parsing does **not** validate ring orientation/validity (permissive input; may re-orient and auto-close on serialization). https://docs.rs/geojson → Caveats

## Rust JTS ports

- No `jts`/`jtsr`/`geo_jts` crate on crates.io. https://crates.io/search?q=jts
- Closest JTS-inspired pure-Rust engine: `wbtopology` 0.2.1 (~2 weeks old), 7.6k downloads — full predicate set, DE-9IM `relate`, `PreparedPolygon`, fixed-scale precision model, `make_valid_polygon`, WKT/WKB; **XY only**, no GeoJSON (I/O via `wbvector`); `GeometryCollection` DE-9IM approximated. https://crates.io/crates/wbtopology

## `geos`

- 11.1.1, MIT, 667k downloads; bindings for GEOS C API (≥ 3.6). https://crates.io/crates/geos
- Default **dynamically links** system GEOS; `static` feature compiles GEOS 3.14.1 from a git submodule (needs C++ toolchain). Self-contained prebuilt binary ⇒ `static` or vendored lib. https://crates.io/crates/geos
- Full predicates + true JTS `PreparedGeometry` + WKT; **strict on validity, prone to crash on invalid input** — validate first. https://crates.io/crates/geos

## Prepared geometry summary

- `geo`: `PreparedGeometry` (R*-tree) implements `Relate` (reuse index for intersects/contains/within); not `Send` until next release. https://docs.rs/geo
- `geos`: JTS `PreparedGeometry`. `wbtopology`: `PreparedPolygon`.

## WGS84 geodesic area (future §14)

- `geo` `GeodesicArea` — Karney (2013) ellipsoidal area/perimeter, signed/unsigned (m²). https://docs.rs/geo/latest/geo/algorithm/geodesic_area/trait.GeodesicArea.html
- `geographiclib-rs` 0.2.7 (georust) — pure-Rust GeographicLib, `PolygonArea` with `Winding`, MSRV 1.70. https://crates.io/crates/geographiclib-rs
- ⚠️ The crate named `geodesic` on crates.io is an unrelated ray-tracer.

## MSRV & license

| Crate | Version | MSRV | License | Downloads |
|---|---|---|---|---|
| geo | 0.33.1 | 1.88 | MIT OR Apache-2.0 | 20.69M |
| geo-types | 0.7.20 | 1.75 | MIT OR Apache-2.0 | 24.58M |
| geojson | 1.0.0 | 1.34 | MIT OR Apache-2.0 | 10.33M |
| geos | 11.1.1 | 1.65 | MIT | 667k |
| geographiclib-rs | 0.2.7 | 1.70 | MIT | 17.94M |
| wbtopology | 0.2.1 | — | MIT OR Apache-2.0 | 7.6k |

## Options

1. **Pure `geo` + `geojson`** — no C deps; DE-9IM-exact via `Relate`; `Validation` gate; `PreparedGeometry` reuse; built-in `GeodesicArea` for future overlap-area. ⚠️ float robust-predicates (not snap-rounded like GEOS); `PreparedGeometry` not `Send` until next release.
2. **`wbtopology`** — `relate`, `PreparedPolygon`, precision model, pure Rust. ⚠️ young (0.2.x), no GeoJSON, XY-only, some DE-9IM approximation.
3. **`geos` static** — battle-tested GEOS + `PreparedGeometry`. ⚠️ C++ build dependency, larger binary, crashes on invalid input.

**Recommended:** pure `geo` + `geojson` — parse GeoJSON → `geo_types::Geometry<f64>`, gate with `Validation::is_valid()`, answer predicates via `Relate`/`PreparedGeometry`, use `GeodesicArea` later for WGS84 overlap area.
