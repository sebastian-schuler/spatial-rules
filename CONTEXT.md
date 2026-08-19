# CONTEXT — Spatial Rules Engine

Single-context repo. Requirements source: `docs/Initial-plan.md`. Decisions: `docs/adr/`. Benchmarks: `docs/benchmarks.md`.

## Glossary

- **Candidate** — a geometry being evaluated against the rules (§4.2).
- **Rule** — an ID, queryable properties, and a geometry, evaluated as a predicate target (§4.1).
- **Rule ID** — the application-supplied identifier of a rule; internally mapped to a numeric `RuleId` (§9).
- **Ruleset** — an immutable, query-optimized collection of rules; the unit that is built, validated, indexed, and atomically replaced (§6, ADR-0007).
- **Spatial predicate** — a boolean relationship between two geometries: `intersects`, `contains`, `within`, `covers`, `covered_by`, `touches`, or `overlaps`, defined by DE-9IM (ADR-0008, ADR-0012).
- **Property predicate** — a boolean test on a rule's properties, expressed in a query's `where` clause (ADR-0003).
- **Query** — one batch evaluation of candidates against a ruleset: a spatial predicate, an optional property `where`, optional excluded rule IDs, and an opt-in overlap computation (`includeOverlap`, ADR-0012).
- **Match** — a candidate satisfying the query against at least one rule; reported per-query as `Matched`, `NotMatched`, or `Invalid` (ADR-0004, ADR-0005).
