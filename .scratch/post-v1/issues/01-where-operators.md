# Richer JSON where operators: $not, $nin, $exists

Type: task
Status: resolved

## Answer

Implemented in `371ad44` (`feat(core): richer where operators $not/$nin/$exists`): field-level `$not`, `$nin`, `$exists` in the `WhereExpr` AST per ADR-0011, per-rule evaluation only (no index extension). Missing field / type mismatch = non-match; `$not`/`$nin` compose under `$and`/`$or`. Tests in `core/tests/query.rs` + `core/tests/engine.rs` and the node smoke pass; `cargo test --workspace` + clippy green.

## Question

Extend the Mongo-style `where` AST (`core/src/where_expr.rs`) per ADR-0011, keeping the hand-rolled JSON walking in `WhereExpr::parse` and the scalar `PropertyValue` unchanged:

- `$not` — field-level, wraps exactly one inner field-op (`{ field: { $not: { $eq: v } } }`); negates the inner predicate under the ADR-0003 missing-property = non-match rule.
- `$nin` — `{ field: { $nin: [...] } }`; a missing field or type mismatch is a **non-match** (documented divergence from Mongo).
- `$exists` — `{ field: { $exists: true|false } }`.

No index extension: new operators are per-rule evaluation; `EqualityIndex` stays equality/`$in` only. `$regex`/`$size` are out of scope. The AST stays the single engine-facing representation (no string front-end).

Tests (core/tests/query.rs, core/tests/engine.rs): each operator's match/non-match including missing-field and type-mismatch cases; nested `$not`; `$not`/`$nin` inside `$and`/`$or`; parity with existing `$ne` behavior. Extend the node smoke test to pass the new operators through.

Run: `cargo test --workspace` and `cargo clippy --workspace --all-targets` — green before commit.
