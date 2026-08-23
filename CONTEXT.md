# CONTEXT — Spatial Rules Engine

Single-context repo. Requirements source: `docs/Initial-plan.md`. Decisions: `docs/adr/`. Benchmarks: `docs/benchmarks.md`.

## Glossary

- **Candidate** — a geometry being evaluated against the rules (§4.2).
- **Rule** — an ID, a top-level integer `priority`, queryable properties, and a geometry, evaluated as a predicate target (§4.1, ADR-0015).
- **Rule ID** — the application-supplied identifier of a rule; internally mapped to a numeric `RuleId` (§9).
- **Priority** — the top-level integer precedence field on a rule: higher wins, a missing field is `0`, ties break by ruleset declaration order (ADR-0015).
- **Applicable rule** — a rule admitted for a candidate under a query: the spatial predicate holds, the `where` clause admits it, and it is not excluded (ADR-0015).
- **Resolution** — a query mode producing, per candidate, the ordered applicable set, its winner, and derived values (ADR-0015).
- **Winner** — the head of the ordered applicable set for a candidate: highest priority, ties by declaration order (ADR-0015).
- **Derived values** — the first-provider-wins merge of applicable rules' properties down the precedence order: each field takes its value from the highest-priority applicable rule that defines it (ADR-0015).
- **Ruleset** — an immutable, query-optimized collection of rules; the unit that is built, validated, indexed, and atomically replaced (§6, ADR-0007).
- **Spatial predicate** — a boolean relationship between two geometries: `intersects`, `contains`, `within`, `covers`, `covered_by`, `touches`, or `overlaps`, defined by DE-9IM (ADR-0008, ADR-0012).
- **Distance predicate** — `withinDistance`: the candidate is within N meters of a rule, measured as the minimum spherical great-circle (haversine) distance, 0 if inside; a metric predicate alongside the DE-9IM set (ADR-0016).
- **Temporal predicate** — `$activeAt`: admits a rule whose window properties (`daysOfWeek` bitmask, `startHour`/`endHour`) contain the query's reference time `at`; shipped as a property-filter predicate (ADR-0017).
- **Property predicate** — a boolean test on a rule's properties, expressed in a query's `where` clause (ADR-0003).
- **Query** — one batch evaluation of candidates against a ruleset: a spatial predicate (or `withinDistance` radius), an optional property `where`, optional excluded rule IDs, an opt-in overlap computation (`includeOverlap`, ADR-0012), and an optional reference time `at` for temporal predicates (ADR-0017).
- **Match** — a candidate satisfying the query against at least one rule; reported per-query as `Matched`, `NotMatched`, or `Invalid` (ADR-0004, ADR-0005).
- **Resolution** — answering "which rule wins, what values apply, and why" for a candidate: the ordered applicable set, its winner (the head), and first-provider-wins derived values (ADR-0015).
- **Applicable set** — the rules admitted for a candidate (the query's spatial predicate holds, the `where` clause admits them, exclusions applied), ordered by precedence (priority desc, ties by declaration order); it is the explanation (ADR-0015).
