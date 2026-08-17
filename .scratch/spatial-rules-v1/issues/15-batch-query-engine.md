# Rust core: batch query engine

Type: task
Status: open
Blocked by: 13, 14

## Question

Build the batch query engine in `spatial-rules-core` (ADR-0002/0003/0004/0005; use the tdd skill):

- Fixed pipeline: spatial bbox filter → property predicates → exact geometry via `Relate` DE-9IM (prepared per worker per the Prepared-geometry answer).
- Mongo-style `where` AST evaluation (`$ne`, `$gt/$gte/$lt/$lte`, `$in`, `$and`/`$or`; missing/mismatch = non-match).
- `Vec<CandidateOutcome>` aligned to input, ready for the binding's `Uint8Array` mask (ADR-0004); invalid candidates stay in the result (ADR-0005).

Query returns aligned outcomes; unit + turf cross-check tests green. Pairs with the Predicate semantics ticket for exact boundary expectations in the test matrix.
