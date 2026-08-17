# Predicate semantics and turf cross-check matrix

Type: grilling
Status: resolved
Blocked by: 01

## Question

Pin exact predicate semantics and the correctness test matrix (§13, §33–§34):

1. **Semantics** — precise definitions of `intersects`, `contains`, `within` for Polygon/MultiPolygon, including boundary behavior: touching boundaries, overlapping boundaries, identical geometries, holes, full containment (§33).
2. **Test matrix** — the enumerated cases in §33 (valid/invalid polygons, holes, touching, identical, containment, disjoint, tiny/huge, country-scale, complex) as concrete named fixtures, with expected results derived from turf.js as the independent reference (§33).
3. **Invalid inputs** — which inputs the predicates themselves must reject vs which are caught earlier (links to Invalid candidate handling).

Locked decision becomes an ADR in `docs/adr/`.

## Answer

Locked (grilling 2026-08-17, recommendations accepted):

- **Semantics:** geo `Relate` DE-9IM is authoritative and documented — `intersects` = matrix ≠ `FF*FF****` (touching → true); `contains` = `T*F**F***`; `within` = `T**F*F***` (contains with args swapped). Boundary conventions are testable.
- **Reference/reconciliation:** turf is an independent oracle, not the spec; geo DE-9IM wins. Pin a JTS-faithful turf major (v6, JSTS-based) in devDependencies; normalize both sides to a shared precision before cross-checking; exclude invalid/degenerate inputs from cross-checks (validated upstream per ADR-0005); remaining disagreements → minimal GeoJSON fixture, confirm vs JTS/GEOS, record as a known-quirk entry.
- **Matrix scope:** this ticket pins the predicate-semantic cases (touching, overlapping boundaries, identical, holes/in-hole, containment, disjoint) as named fixtures; the broader §33 list stays with the ingestion/query-engine build tickets.
- **Invalid inputs:** predicates assume valid inputs; invalids are rejected upstream (rules at compile, candidates at ingestion → per-candidate `invalid`); no hot-path re-validation.

Assets: [research/11-predicate-semantics.md](../research/11-predicate-semantics.md) · [ADR-0008](../../../docs/adr/0008-predicate-semantics.md).
