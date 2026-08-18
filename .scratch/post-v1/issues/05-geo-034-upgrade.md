# geo 0.34 upgrade: Send PreparedGeometry in Arc<Ruleset>

Type: task
Status: needs-triage

## Question

Opportunistic upgrade (ADR-0010 follow-up), gated on **geo 0.34 being published to crates.io** — not yet released as of 2026-08-18 (latest: 0.33.1; the `Rc → Arc` / `PreparedGeometry`-`Send` fix is on geo main only). No git-dep pinning (keeps the crate publishable).

When 0.34 ships:

- Bump the workspace `geo` pin; verify the used API surface (`PreparedGeometry::from`, `relate`, `is_*` methods, `Rect`, `BoundingRect`, `Validation`) — the only known 0.34 delta is the new `InvalidPolygon::InteriorNotSimplyConnected` variant (non-breaking here; we only `{:?}`-format errors).
- Delete the `thread_local!` prepared-geometry cache (`core/src/ruleset.rs`); store prepared geometries in the shared `Arc<Ruleset>` (one geometry clone process-wide instead of per-thread).
- Re-run the criterion ladder (E/F), the turf cross-check, and node smoke; update ADR-0010 and `docs/benchmarks.md`.
- Regression: behavior-identical queries; memory unchanged or improved.

Re-triage to `ready-for-agent` when geo 0.34 is on crates.io.
