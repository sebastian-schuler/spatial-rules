# Geometry stack: geo + geojson over GEOS/JTS

The spatial core is built on the pure-Rust `geo` 0.33 (georust) ecosystem — GeoJSON is parsed with the `geojson` crate into `geo_types::Geometry<f64>`, spatial predicates are answered uniformly via `Relate` DE-9IM, and invalid rule geometries are rejected at ruleset compile via `geo::Validation`. Pure Rust was chosen over a GEOS-backed stack so prebuilt native binaries need no C/C++ system dependency (§26–§27 of `docs/Initial-plan.md`), and over the young `wbtopology` JTS port for ecosystem maturity (20M vs 7.6k downloads) and GeoJSON support.

## Considered Options

- `geos` (static GEOS): battle-tested predicates and `PreparedGeometry`, but a C++ static build, larger binaries, and crashes on invalid input — a worse fit for self-contained prebuilt distribution.
- `wbtopology` (JTS-inspired): fixed-scale precision model + `PreparedPolygon`, but 0.2.x, XY-only, no GeoJSON, and approximate DE-9IM for GeometryCollections.
