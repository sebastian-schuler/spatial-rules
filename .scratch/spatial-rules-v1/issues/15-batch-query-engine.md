# Rust core: batch query engine

Type: task
Status: resolved
Blocked by: 13, 14

## Question

Build the batch query engine in `spatial-rules-core` (ADR-0002/0003/0004/0005; use the tdd skill):

- Fixed pipeline: spatial bbox filter → property predicates → exact geometry via `Relate` DE-9IM (prepared per worker per the Prepared-geometry answer).
- Mongo-style `where` AST evaluation (`$ne`, `$gt/$gte/$lt/$lte`, `$in`, `$and`/`$or`; missing/mismatch = non-match).
- `Vec<CandidateOutcome>` aligned to input, ready for the binding's `Uint8Array` mask (ADR-0004); invalid candidates stay in the result (ADR-0005).

Query returns aligned outcomes; unit + turf cross-check tests green. Pairs with the Predicate semantics ticket for exact boundary expectations in the test matrix.

## Answer

Built the batch query engine in `spatial-rules-core`, committed to `main`.

**`Ruleset::query`** (`core/src/ruleset.rs`): `query(&[Candidate], &Query) -> Vec<CandidateOutcome>`, one outcome per candidate in input order (ADR-0004). Fixed pipeline (§15): candidate geometry gate (unsupported type / invalid geometry → `Invalid`, never a batch failure, ADR-0005) → spatial bbox filter via the `SpatialIndex` → per-rule `exclude_rule_ids` skip (`HashSet`, unknown ids ignored) → property predicate → exact `Relate` DE-9IM. `spatial_predicate_holds` relates `candidate` to `rule` (directional for `contains`/`within`). Prepared geometries are deferred to the harness ladder (E/F, research 03) — plain `Relate` is used here for correctness.

**`SpatialPredicate` + `Query`** (`core/src/query.rs`): `intersects`/`contains`/`within` via `FromStr` (else `SR_UNSUPPORTED_SPATIAL_PREDICATE`); `Query { spatial, where_clause, exclude_rule_ids }` with `Query::from_json` parsing `{ spatial: { predicate }, where, excludeRuleIds }` (`SR_INVALID_QUERY` for malformed top-level shape).

**`WhereExpr`** (`core/src/where_expr.rs`): Mongo-style subset — implicit top-level `AND`, plain equality, `$ne`, `$gt/$gte/$lt/$lte`, `$in`, `$and`/`$or`. Missing property or type mismatch = non-match (even `$ne`); numeric range compares across `Int`/`Float`. Unknown `$op` → `SR_UNSUPPORTED_PROPERTY_OPERATOR`; malformed predicate (bad arity, non-array `$in`, array outside `$in`, non-scalar operand) → `SR_INVALID_PROPERTY_PREDICATE`.

**`CandidateOutcome`**: `Matched { rule_ids }` / `NotMatched` / `Invalid { reason }` — numeric rule ids, ready for the binding's `Uint8Array` mask (0/1/2) and rich string-id API (ADR-0004).

**Tests**: 24 new (`core/tests/query.rs`), 62 total green, clippy clean. Seams tested: aligned outcomes; intersects/contains/within directionality; touching-edge intersects-but-not-contains; identical geometry matches all three; candidate-inside-hole is disjoint (exact step); `where` equality/missing/`$ne`-type/range/`$in`/`$and`/`$or`; `excludeRuleIds` + unknown-id ignored; invalid and unsupported candidates stay in the result; malformed query/predicate/operator/spatial error codes. The live turf.js cross-check suite is owned by ticket 12 (benchmark harness); this ticket's DE-9IM boundary matrix is hand-computed from ADR-0008.

Run: `cargo test --workspace` / `cargo clippy --workspace --all-targets`.

