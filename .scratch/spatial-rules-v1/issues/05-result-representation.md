# Result representation and compact mask formats

Type: grilling
Status: resolved

## Question

Decide the result model (§42 item 10; §18–§19):

1. **Core result** — candidate↔rule relationship model (`Match { candidate_id, rule_ids }` or richer), extensible later with predicate/overlap fields.
2. **Compact filtering path** — what the Node binding returns for the hot path: candidate indices, bitset/typed-array mask, or rule-ID arrays (§18–§19). Which minimizes allocation and JS↔Rust crossing for the ~1,000-candidate filter use case.
3. **Diagnostic API** — a richer mode exposing full candidate-to-rule relationships for debugging.
4. **Invalid candidates in results** — how `invalid` status shows up in each mode (§34).

Locked decision becomes an ADR in `docs/adr/`.

## Answer

Locked (grilling 2026-08-14, recommendations accepted):

- **Core result:** `Vec<CandidateOutcome>` aligned to input order — `Matched { rule_ids } | NotMatched | Invalid { reason }`; internal numeric `RuleId` (`0..n-1`, §9).
- **Compact hot path:** a single `Uint8Array` mask of length N — `0` = no match, `1` = matched, `2` = invalid; one allocation, no per-feature objects. A `Uint32Array` of matched indices can be added later if benchmarks justify it.
- **Rich/diagnostic API:** per-candidate objects `{ candidateId, outcome, ruleIds: string[], error? }` for all candidates, with original string rule IDs and the invalid reason.

Asset: [ADR-0004](../../../docs/adr/0004-result-model.md).
