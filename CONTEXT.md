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
- **Resolution** — answering "which rule wins, what values apply, and why" for a candidate: the ordered applicable set, its winner (the head), and first-provider-wins derived values (ADR-0015).
- **Applicable set** — the rules admitted for a candidate (the query's spatial predicate holds, the `where` clause admits them, exclusions applied), ordered by precedence (priority desc, ties by declaration order); it is the explanation (ADR-0015).
