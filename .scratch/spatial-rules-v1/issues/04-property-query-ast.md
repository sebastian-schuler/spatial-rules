# Property query AST, typed storage, and property indexes

Type: grilling
Status: resolved

## Question

Design the property-query side (§42 item 9; §10–§12):

1. **Typed property storage** — compact typed structures for string / integer / float / boolean / null (§10); no arbitrary nested objects.
2. **Query AST** — operators `= != > >= < <= IN` and logical `AND`/`OR` (§11), shaped so a SQL-like syntax could be added later without engine changes.
3. **Property indexes** — which properties get inverted indexes at ruleset compile (§12), and the planner's rule for ordering property vs spatial filtering per query (§17).

Constraints: predicates filter candidate rules before exact geometry work; the AST must validate cleanly (invalid operator/type → stable error, §35). Locked decision becomes an ADR in `docs/adr/`.

## Answer

Locked (grilling 2026-08-14, recommendations accepted):

- **Typed storage:** `PropertyValue { Null, Bool(bool), Int(i64), Float(f64), Str(...) }` per rule; JSON numbers → `Int` when integral, else `Float`.
- **Query AST:** Mongo-style subset — top-level implicit `AND`; plain value = equality; `$ne`, `$gt/$gte/$lt/$lte`, `$in`, `$and`/`$or` arrays. The internal AST stays engine-private so a SQL-like syntax can map onto it later.
- **Missing property or type mismatch** → non-match (false), including `$ne`; malformed query (unknown operator, bad `$in`) → query-build error (§35).
- **Property indexes:** equality + `$in` indexes built for every property at ruleset compile; range predicates scanned.
- **Planner order:** fixed spatial-bbox → property → exact geometry (§15), with a hook for a cost-based planner later.

Asset: [ADR-0003](../../../docs/adr/0003-property-query-model.md).
