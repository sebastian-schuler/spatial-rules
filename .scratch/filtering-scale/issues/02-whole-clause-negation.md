# Whole-clause negation: top-level `$nor` on `where`

Type: task
Status: resolved

## Answer

Implemented (filtering-scale, 2026-08-19): `$nor: [<where>...]` in the `where`
AST (`core/src/where_expr.rs`) — matches when zero inner clauses match,
composable under `$and`/`$or`, per-rule evaluation only (no index extension:
`property_index.rs` answers `Nor` as non-indexable). Missing-field/type-mismatch
inner clauses are non-matches per ADR-0003, so a `$nor` over them matches; the
divergence from Mongo is documented in ADR-0011 (amended). Tests:
`core/tests/query.rs` (empty/single/multiple, missing-field, type-mismatch,
nesting, client-side-negation parity), `core/tests/engine.rs`,
`node/test/smoke.mjs`. `cargo test --workspace` + clippy green.

## Question

The `where` AST (`core/src/where_expr.rs`) supports field-level `$not` only
(ADR-0011); whole-clause negation — "NOT (fieldA = v OR fieldB = v)" — must
today be expressed client-side by flipping the predicate. Add top-level boolean
structure so exemption logic ("user is exempt unless restricted") can be stated
engine-side, in the permission/library filtering use case.

Scope:
- `$nor: [ <where> ... ]` — matches when **zero** inner clauses match, under the
  ADR-0003 rule that a missing property or type mismatch is a non-match.
- Composable under `$and`/`$or` for parity with `$not`/`$nin` (ADR-0011).
- Keep the AST the single engine-facing representation (no string query
  front-end); no index extension — per-rule evaluation like `$not`/`$nin`.

Tests (`core/tests/query.rs`, `core/tests/engine.rs`): `$nor` with empty,
single and multiple inner clauses; missing-field and type-mismatch inner
clauses; nesting under `$and`/`$or`; parity with client-side negation of the
same predicate. Node smoke passes `$nor` through.

Run: `cargo test --workspace` and `cargo clippy --workspace --all-targets`
green before commit.
