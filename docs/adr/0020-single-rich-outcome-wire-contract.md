# One rich-outcome wire-contract seam across all bindings

The rich-outcome wire contract — the per-candidate JSON a binding hands off
for `query`/`resolve` (`ruleIds`/`winner`/`values`/`applicable`/`aggregate`/
`overlaps`, plus the `{outcome: notMatched}` and `{outcome: invalid, reason}`
shapes) — lives in **`spatial-rules-bindings-common`**, consumed by all three
language bindings: Node (napi-rs), wasm, and Python (PyO3). Its public
interface is the two batch helpers `query_rich_json` / `resolve_rich_json`
(plus `parse_query`, `report_to_json`, `spatial_error_message`); the
per-outcome serializers (`candidate_outcome_to_json`,
`resolution_outcome_to_json`, `aggregate_json`) are **internal** implementation
detail behind them.

This follows the seam rule in the codebase-design vocabulary — "one adapter
means a hypothetical seam, two means a real one": node, wasm, and python all
serialize identical payloads, so the contract must be specified once. ADR-0019
scoped node's inline copy as "out of scope for that effort"; this ADR supersedes
that note — node now depends on `spatial-rules-bindings-common` and no longer
carries its own copy.

## Why the seam

- **Leverage**: one interface, three adapters. A wire-contract change (a new
  outcome field, a shape tweak) is edited once and reaches every binding.
- **Locality**: the contract concentrates in one module, so a divergence (node
  reshaping `applicable` while wasm doesn't) cannot silently ship.
- **Interface is the test surface**: the contract is guarded by the module's own
  tests through `query_rich_json` / `resolve_rich_json`, not by each binding
  re-testing its private serialization.

## Shape

The batch helpers take `(&Ruleset, &[Outcome])` and return the assembled JSON as
`String` — infallible, since the payloads are built from domain types that
always serialize. The per-binding input normalization (node `Buffer`, wasm
`&str`, python `str | bytes | dict`) and the host-language output marshalling
stay in each binding; only the payload assembly is shared.

## What is NOT in the seam

The candidate and query are not needed for serialization: the aggregate is
computed by the core engine and carried on the outcome (ADR-0018), and the
overlaps and rule ids ride the outcome too — so the wire layer is a pure
outcome→JSON serializer. Host-specific error adapters (`spatial_error_to_napi*`,
Python `to_pyerr`) and the ruleset lifecycle (`Engine` vs owned `Ruleset`) stay
in each binding.

## Considered Options

- **Keep node's inline copy** — the ADR-0019 scoping note. Rejected: two
  implementations of the same wire contract, so a change to one can drift from
  the other without a test catching it.
- **Deduplicate serializers only (no batch helpers)** — move node's functions
  into `bindings-common` but leave the zip-and-serialize loop in each binding.
  Rejected: the assembly loop was repeated six times; the batch helper is the
  seam, not the individual serializer.
- **Return `serde_json::Value` instead of `String`** — friendlier to a
  hypothetical first-class-object binding. Rejected: all three adapters put a
  `String` on the wire today (python builds one then parses it); returning a
  `Value` would push a serialization fact back onto every caller.
- **Keep the per-outcome serializers public** — Rejected: they exist only to
  serve the batch helpers; exposing them widens the interface without adding
  leverage. They stay as internal seams covered by the module's own tests.

## Out of scope

- Replacing the per-binding input normalization / output marshalling.
- Any change to `spatial-rules-core` itself.

## Amended 2026-09-01 (aggregate into the core)

The aggregate moved from a binding-side `AggregateSpec::compute` call into the
core engine, carried on the matched/resolved outcome; the serializers now read
`outcome.aggregate`, and the batch helpers dropped the `candidates`/`query`
parameters. The wire layer is now purely an outcome→JSON serializer, and the
mask paths are untouched (they never build the outcome enum).
