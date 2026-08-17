# Prepared-geometry options in the chosen stack

Type: research
Status: resolved
Blocked by: 01

## Question

Investigate how prepared/precomputed geometry acceleration can work in the stack chosen by Geometry stack (§42 item 3; §16):

- Does the chosen geometry library offer a prepared-geometry API (e.g. JTS-style PreparedGeometry, cached orientation/envelope, indexed intersection)?
- If not, what do established Rust spatial projects do instead — pre-triangulation, cached envelopes, internal indexing, or accepting per-query cost?
- What is the expected win for ~30 prepared rules vs ~1,000 candidates per request, and which options are worth benchmarking in the ladder (§32)?

Findings against primary sources (crate docs, source code, upstream issues), written to `research/03-prepared-geometries.md` and linked here. The answer records the options and a recommendation for the follow-up decision.

## Answer

Findings with per-claim sources: [research/03-prepared-geometries.md](../research/03-prepared-geometries.md).

Key facts:

- `geo::PreparedGeometry` accelerates only `Relate` (caches self-noding + R-tree); prepared relate measured ≈**17× faster** than unprepared (49 ms vs 843 ms repeated). It is `!Send`/`!Sync` in released 0.33.1; a `Send` fix is merged to `main` (PR #1571, ~2026-08-13) and unreleased — ships in 0.34.x, still not `Sync`.
- `MonotoneChain*` are Send+Sync but cover only `intersects` and `contains_properly` (both sides must be monotone) — no plain `contains`/`within`.
- `IntervalTreeMultiPolygon` is point/coord containment only — not applicable to polygon-vs-polygon.

Recommendation (feeds the harness task, which owns ladder E/F):

- Store plain `Polygon`/`MultiPolygon` in the shared `Arc<Ruleset>` (Send+Sync).
- Prepare lazily **per worker**: build `PreparedGeometry` inside the thread that uses it (0.33.1 constraint), relate one-sided `prepared_rule.relate(&candidate)`; answer `within` as `candidate.relate(&prepared_rule).is_within()` so the rule stays on the prepared side.
- Revisit at geo 0.34 (`PreparedGeometry` becomes `Send`, still not `Sync`): prebuild once at ruleset compile and clone per worker.
- Skip `MonotoneChain*` and `IntervalTreeMultiPolygon` — they don't cover the three required predicates.

The final prepared-vs-unprepared adoption decision is made by the harness task's ladder E/F numbers (§32), not here.
