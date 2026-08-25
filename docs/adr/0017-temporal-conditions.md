# Temporal conditions: query-supplied `at`, whole-clause `$activeAt` over scalar rule window properties

P2 temporal conditions ship as the roadmap's "property-filter predicate first": rules declare their active windows as **ordinary typed scalar properties** — `daysOfWeek` (Int bitmask, Mon=1 … Sun=64), `startHour`/`endHour` (Int 0..=23) — with no reserved keys and no rule-schema change (`Rule` stays `{id, properties, geometry, priority}`; arrays/objects remain rejected at ingestion). The engine stays pure and deterministic (no wall clock): the query supplies the reference time as a top-level **`at`** (ISO-8601), parsed to a naive-local day-of-week + time-of-day. `at` is required (`SR_INVALID_QUERY`) whenever a temporal predicate is present; a present-but-unused `at` is parsed and validated.

The predicate is **`$activeAt`**, a `WhereExpr` variant composable at any clause position (`$and`/`$or`/`$nor`), naming the rule's window fields explicitly: `{ "$activeAt": { "daysOfWeek": "<field>", "startHour": "<field>", "endHour": "<field>" } }`. Admission is start-inclusive / end-exclusive, wraps midnight when `startHour > endHour`; a missing temporal field or `daysOfWeek = 0` → non-match (consistent with the existing missing-property = non-match rule); `startHour == endHour` → an empty window (never active). It composes through the existing whole-clause dispatch (`$nor` precedent) and evaluates per-rule — only `Eq`/`$in` are indexed today, so windows scan, and first-class temporal indexing stays in fog.

## Considered Options

- **Reserved window keys on rules** — rejected: collides with the "any typed property" rule model; an application `daysOfWeek` meaning something else would silently become a window.
- **Field-level operator on one designated property** — rejected: a compound start/end window cannot be one scalar value.
- **Engine wall clock** — rejected: the query path must stay deterministic and pure; time travels with the request.
- **Offset-aware `at` parsing** — deferred: v1 windows are local-frame; timezone/offset handling is documented additive.