# Streaming geofencing: watch(candidate) → ENTER/STAY/EXIT

Type: grilling
Status: ready-for-human

## Question

Per `docs/roadmap.md` fog: "**Streaming geofencing** — `watch(candidate)`
emitting ENTER/STAY/EXIT. A different product surface (stateful, per-candidate
subscriptions); revisit after P1/P2 prove the decision model." P1 and P2 are
shipped, so this is the strongest unblocked feature — and it is a **new,
stateful** surface (the engine is currently stateless: whole-buffer,
batch-per-request). It needs a design grill before implementation.

**The design tree (the grill frontier):**

- **Q1 — The surface**: a stateful `watch(candidate)` subscription on a
  `SpatialRuleset` that emits ENTER/STAY/EXIT events as a candidate's relation
  to the ruleset changes, vs a stateless "did this candidate transition since
  the last snapshot" check the caller drives.
- **Q2 — The state model**: per-candidate snapshot of the previous
  applicable-set (or winner). What is an event — ENTER (no applicable rule →
  some), EXIT (some → none), and is a change in the **winner** (priority) or
  the applicable composition a distinct event? Does a candidate moving from
  zone A (priority 10) to zone B (priority 20) emit EXIT/ENTER or a new event?
- **Q3 — Re-evaluation**: when does a watch re-evaluate — on a schedule, on
  ruleset `replace`, on explicit poll, or a push/stream? How does the query a
  watch carries (`spatial`/`where`/`at`/`withinDistance`/`aggregate`) interact
  with time (temporal `$activeAt` makes ENTER/STAY/EXIT time-dependent — a zone
  whose window opens/closes changes the state without the candidate moving)?
- **Q4 — Semantics reuse**: the resolution applicable set (ADR-0015) is the
  natural state key; confirm the event grammar is over it, not a new predicate.
- **Q5 — Lifecycle**: unsubscribing, many candidates (resource bounds), the
  sync-vs-async surface (a `watch` that pushes vs `poll`).

Once grilled, this graduates to `.scratch/streaming-geofencing/` with an ADR
and implementation tickets.

## Comments

> *Roadmap fog item, now unblocked by the shipped decision model.*

## Agent Brief

**Category:** enhancement
**Summary:** Design and then implement a stateful geofencing watch surface (ENTER/STAY/EXIT) over the resolution applicable set.

**Current behavior:** The engine is stateless and batch-per-request; there is no subscription or event surface.

**Desired behavior:** A `watch`/poll surface emitting ENTER/STAY/EXIT per candidate as its relation to the ruleset (or its winner/applicable set) changes, reusing the resolution primitive as the state key.

**Key interfaces:** `SpatialRuleset` (wrapper), the resolution applicable set (core), a new state-keeping seam.

**Acceptance criteria (post-grill):**
- [ ] The event grammar (ENTER/STAY/EXIT, winner-change) is decided and recorded
- [ ] The re-evaluation/lifecycle model is decided
- [ ] Implementation tickets exist in `.scratch/streaming-geofencing/`

**Out of scope:**
- Any change to the stateless batch surfaces
- Temporal indexing (separate fog item)