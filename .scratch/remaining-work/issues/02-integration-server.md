# Integration server: resolve / withinDistance / temporal / aggregation endpoints

Type: task
Status: resolved

## Question

`integration/server.mjs` exposes only `/query` (match mask) and `/replace`. The
surfaces shipped since (resolution, `withinDistance`, temporal `$activeAt`,
aggregation) are core/wrapper-tested but not exercised through the HTTP
integration server — the consumer-facing story over the wire is incomplete.

Add:

- `/resolve` — returns the resolution mask (and, with a rich flag, the
  per-candidate `{outcome, winner, values, applicable, aggregate}` JSON).
- Let the existing query object carry the new members end to end — `spatial:
  {predicate: "withinDistance", distance}`, `at` + `$activeAt`, and `aggregate`
  — so `/query` and `/resolve` both honor them through the same `Query` parse.
- Extend `integration/smoke.mjs` to assert the resolve mask and at least one
  distance/temporal/aggregate query against the running server.

The byte-oriented `/queryRaw` path must stay unchanged.

Run: `cargo test --workspace`, clippy, and the integration smoke
(`bun run bench server` then `bun run bench smoke`).

## Comments

> *Consumer-facing completion of the P1/P2/aggregation story.*

> *Resolved 2026-08-24 (commit `a012059`): `integration/server.mjs` gains
> `/resolve` (compact mask; `rich: true` → per-candidate
> `{outcome, winner, values, applicable, aggregate}`) and a `rich` flag on
> `/query` (match outcomes carrying `aggregate`/`includeOverlap`); both honor
> the full query shape (`withinDistance`+`distance`, `at`+`$activeAt`,
> `aggregate`) through the same `Query` parse, with 400 `SR_*` errors.
> `/queryRaw` is unchanged. `integration/smoke.mjs` asserts the resolve mask,
> distance/temporal/aggregate cases (exact hand-computed literals over a
> controlled `/replace` ruleset, plus production-dataset parity checks) and
> `/queryRaw` byte-identity. `cargo test --workspace`, clippy, node typecheck,
> and server+smoke green.*

## Agent Brief

**Category:** enhancement
**Summary:** Expose resolution, distance, temporal, and aggregation through the integration HTTP server and its smoke test.

**Current behavior:** `/query` returns only the match mask; the new surfaces have no HTTP integration coverage.

**Desired behavior:** `/resolve` (mask + optional rich outcomes), the new query members passing through `/query`, and smoke assertions for them.

**Key interfaces:** `integration/server.mjs`, `integration/smoke.mjs`, `benchmarks.json` (server port/paths), the wrapper's `SpatialRuleset`.

**Acceptance criteria:**
- [ ] `/resolve` returns the resolution mask, and a rich form returns `{outcome, winner, values, applicable, aggregate}`
- [ ] A `withinDistance` query through `/query` returns the correct mask
- [ ] An `at` + `$activeAt` query through `/query` returns the correct mask
- [ ] An `aggregate` query returns the aggregate payload
- [ ] `integration/smoke.mjs` asserts at least the resolve mask and one distance/temporal/aggregate case
- [ ] `/queryRaw` is unchanged; server + smoke pass

**Out of scope:**
- Benchmark ladder additions (ticket 01)
- Any engine or wrapper change