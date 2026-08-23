# P1 — From matches to decisions — Spec

The roadmap gate: "No P1 implementation starts before the precedence/conflict-resolution model is decided." Settled by the 2026-08-23 grilling session into ADR-0015. P1 lands its three features together: rule priority/resolution, derived values, and explainability.

## Scope

1. **Top-level priority field** — `priority` as a GeoJSON foreign member on rule features; hoisted at compile; additive to the ADR-0013 canonical form. Ticket 01.
2. **Resolution evaluation** — ordered applicable set → winner + first-provider-wins values (collect-then-resolve). Ticket 02.
3. **Explanation** — flat per-rule `{ruleId, priority, spatialMatched, propertyMatched}` riding the ordered set. Ticket 03.
4. **`resolve()` / `resolveAsync()` API** — chainable `ResolutionResult` mirroring ADR-0014. Ticket 04.
5. **Test suite** — property tests + determinism; turf cannot oracle resolution. Ticket 05.

## Sequencing

Data model (01) → resolution evaluation (02) → explanation shape (03) → binding/API (04); tests (05) parallel 02. Blocking: 02 blocked by 01; 03 by 02; 04 by 02+03; 05 by 02.

## Cross-cutting

- ADR-0015 is the authoritative decision; tickets cite it.
- `query()` and the hot mask path are untouched throughout.
- `Rule` gains `priority: i64` (`#[serde(default)]` = 0) — canonical round-trip and old-canonical compatibility are part of ticket 01.

## Explicitly deferred (additive later, no shape change)

- `resolveFields` subset selection.
- First-class `action: allow|deny` (expressible as data on priority).
- Geometric-specificity dimension (documented extension path, not v1).
- Field-by-field property predicate traces.
- Priority-ordered early-exit (collect-then-resolve is the ADR stance).

## Ticket index

- `issues/01-priority-field.md` — needs-triage
- `issues/02-resolution-evaluation.md` — needs-triage (blocked by 01)
- `issues/03-explanation-shape.md` — needs-triage (blocked by 02)
- `issues/04-resolve-api.md` — needs-triage (blocked by 02, 03)
- `issues/05-resolution-tests.md` — needs-triage (blocked by 02)