# Ruleset build cancellation and replacement progress

Type: grilling
Status: resolved

## Question

Decide the replacement lifecycle control surface (§42 item 13; §37):

1. **Cancellation** — can an in-flight ruleset build be cancelled (weekly changes, ~30 rules — is cancellation over-engineering, or required for graceful shutdown/deployments)?
2. **Progress** — does the application need build progress signals, or is "built → atomic swap" enough?
3. **Request-path safety** — confirm the request path keeps using the old ruleset until the new one is fully parsed/validated/indexed (§37), and what the API exposes for observability (last swap time, build duration).

Locked decision becomes an ADR in `docs/adr/`.

## Answer

Locked (grilling 2026-08-17, recommendations accepted):

- **Cancellation:** none in v1 — builds are fast (~30 rules, weekly swaps); revisit only if rule counts grow to where a build could take long enough to matter (§4.1 keeps the door open).
- **Progress:** no per-step callback; coarse observability only — `lastSwapTime`, `buildDurationMs`, active ruleset id/count.
- **Replacement model:** build fully off the hot path, then atomic `Arc` swap — the request path keeps using the old ruleset until publication; old rulesets stay alive while in-flight queries reference them and are released when none do (§25). The sync/async surface of `replace()` is left to the Sync vs async ticket.

Asset: [ADR-0007](../../../docs/adr/0007-ruleset-replacement.md).
