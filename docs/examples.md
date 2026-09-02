# Real-world examples

This walkthrough builds one application end to end — a city's delivery/parking
rules engine — and exercises every shipped feature in one place: matching,
property `where` filters, temporal conditions, geofencing, resolution
(winner + derived values), aggregation, async evaluation, and hot reload.

Everything is Node/Bun wrapper code; the exact query shapes are documented in
the root [README](../README.md).

## The scenario

A city publishes spatial rules that delivery trucks must satisfy. Each rule is
a zone with:

- a **priority** (top-level `priority` field — ADR-0015): when zones overlap,
  the highest priority wins;
- **derived values** (typed `properties`): the effective `speedLimit`, `taxRate`,
  and `kind` at a location;
- a **temporal window** (`daysOfWeek` bitmask + `startHour`/`endHour` — ADR-0017):
  when the zone is active.

Trucks (GPS points) are evaluated against the zones.

## 1. The rules

```ts
import { SpatialRuleset } from 'spatial-rules';

const cityZones = {
  type: 'FeatureCollection',
  features: [
    {
      type: 'Feature', id: 'school-zone', priority: 10,
      properties: {
        kind: 'school', speedLimit: 30, taxRate: 0.21,
        daysOfWeek: 31,      // Mon–Fri (bitmask: Mon=1 … Sun=64)
        startHour: 9, endHour: 17,
      },
      geometry: { type: 'Polygon', coordinates: [[[0, 0], [0, 10], [10, 10], [10, 0], [0, 0]]] },
    },
    {
      type: 'Feature', id: 'downtown', priority: 5,
      properties: {
        kind: 'commercial', speedLimit: 50, taxRate: 0.10,
        daysOfWeek: 127,    // every day
        startHour: 0, endHour: 24,
      },
      geometry: { type: 'Polygon', coordinates: [[[5, 5], [5, 15], [15, 15], [15, 5], [5, 5]]] },
    },
    {
      type: 'Feature', id: 'no-delivery', priority: 20,
      properties: { kind: 'restricted' },   // no window -> never temporally active
      geometry: { type: 'Polygon', coordinates: [[[8, 8], [8, 12], [12, 12], [12, 8], [8, 8]]] },
    },
    {
      type: 'Feature', id: 'riverfront', priority: 15,
      properties: {
        kind: 'commercial', speedLimit: 40, taxRate: 0.15,
        daysOfWeek: 127, startHour: 0, endHour: 24,
      },
      geometry: { type: 'Polygon', coordinates: [[[20, 20], [20, 30], [30, 30], [30, 20], [20, 20]]] },
    },
  ],
};

const ruleset = new SpatialRuleset(cityZones);
```

## 2. Basic matching

```ts
const trucks = {
  type: 'FeatureCollection',
  features: [
    { type: 'Feature', id: 'truck-1', properties: {}, geometry: { type: 'Point', coordinates: [6, 6] } },
    { type: 'Feature', id: 'truck-2', properties: {}, geometry: { type: 'Point', coordinates: [9, 9] } },
    { type: 'Feature', id: 'truck-3', properties: {}, geometry: { type: 'Point', coordinates: [25, 25] } },
    { type: 'Feature', id: 'truck-4', properties: {}, geometry: { type: 'Point', coordinates: [40, 40] } },
  ],
};

const matched = ruleset.query(trucks, { spatial: { predicate: 'intersects' } });
matched.mask();          // Uint8Array [1, 1, 1, 0]
matched.count();         // 3
matched.summary();       // { matched: 3, notMatched: 1, invalid: 0 }
matched.indices();       // Uint32Array [0, 1, 2]
matched.invalidIndices(); // Uint32Array []

// Which rules matched each truck (original string rule ids).
const rich = JSON.parse(matched.toOutcomesJson());
// rich[0] = { outcome: 'matched', ruleIds: ['school-zone', 'downtown'] }
// rich[1] = { outcome: 'matched', ruleIds: ['school-zone', 'downtown', 'no-delivery'] }
// rich[2] = { outcome: 'matched', ruleIds: ['riverfront'] }
// rich[3] = { outcome: 'notMatched' }
```

## 3. Property filters

The `where` clause admits only rules whose typed properties match (Mongo-style
operators: `$eq`/`$ne`/`$gt`/`$gte`/`$lt`/`$lte`/`$in`/`$nin`/`$exists`/`$not`,
composed with `$and`/`$or`/`$nor`):

```ts
// Only commercial zones (downtown, riverfront).
const commercial = ruleset.query(trucks, {
  spatial: { predicate: 'intersects' },
  where: { kind: 'commercial' },
}).toOutcomesJson();

// Zones whose taxRate is at least 0.15 (school-zone, riverfront).
const taxed = ruleset.query(trucks, {
  spatial: { predicate: 'intersects' },
  where: { taxRate: { $gte: 0.15 } },
});
```

## 4. Temporal conditions

Rules declare windows as properties; the query supplies the reference time with
`at` (ISO-8601) and admits rules via the whole-clause `$activeAt` operator.
`at` is required whenever `$activeAt` is used.

```ts
// Monday 10:00 — school-zone (Mon–Fri 9–17) and every-day zones are active;
// no-delivery (no window) never is.
const mondayMorning = ruleset.query(trucks, {
  spatial: { predicate: 'intersects' },
  at: '2026-08-24T10:00',
  where: {
    $activeAt: { daysOfWeek: 'daysOfWeek', startHour: 'startHour', endHour: 'endHour' },
  },
});
// truck-2 sits in school-zone ∩ downtown ∩ no-delivery, but no-delivery is
// excluded by the window: ruleIds = ['school-zone', 'downtown']

// Saturday 10:00 — school-zone is inactive; only every-day zones admit.
const saturday = ruleset.query(trucks, {
  spatial: { predicate: 'intersects' },
  at: '2026-08-29T10:00',
  where: { $activeAt: { daysOfWeek: 'daysOfWeek', startHour: 'startHour', endHour: 'endHour' } },
});
```

## 5. Geofencing

`withinDistance` is a metric predicate: the candidate is within N meters of the
rule (minimum haversine distance, 0 if inside — ADR-0016).

```ts
// Which trucks are within 100 m of any zone (a GPS fix near a curb/zone edge).
const nearby = ruleset.query(trucks, {
  spatial: { predicate: 'withinDistance', distance: 100 },
});
// trucks 1–3 are inside their zones (distance 0 → within any radius); truck-4
// is not: mask [1, 1, 1, 0]

// Combine with a property filter: commercial zones only.
const nearbyCommercial = ruleset.query(trucks, {
  spatial: { predicate: 'withinDistance', distance: 100 },
  where: { kind: 'commercial' },
});
```

## 6. Resolution — the decision

`resolve()` answers "which rule wins, what values apply, and why" per candidate:
the ordered **applicable** set, its **winner** (highest priority), and
first-provider-wins **values**.

```ts
const decisions = ruleset.resolve(trucks, {
  spatial: { predicate: 'intersects' },
  at: '2026-08-24T10:00',
  where: {
    $activeAt: { daysOfWeek: 'daysOfWeek', startHour: 'startHour', endHour: 'endHour' },
  },
});
decisions.mask();    // Uint8Array [1, 1, 1, 0]
decisions.summary(); // { resolved: 3, notResolved: 1, invalid: 0 }

const decision = JSON.parse(decisions.toJson());
// decision[0] = {
//   outcome: 'resolved',
//   winner: 'school-zone',                        // priority 10 > downtown 5
//   values: { kind: 'school', speedLimit: 30, taxRate: 0.21,
//             daysOfWeek: 31, startHour: 9, endHour: 17 },
//   applicable: [
//     { ruleId: 'school-zone', priority: 10, spatialMatched: true, propertyMatched: true },
//     { ruleId: 'downtown',     priority: 5,  spatialMatched: true, propertyMatched: true },
//   ],
// }
// decision[2] = { outcome: 'resolved', winner: 'riverfront', ... }
// decision[3] = { outcome: 'notMatched' }
```

## 7. Aggregation — analytics

The `aggregate` query member computes per-candidate analytics over the
applicable set (count, min/max/sum/avg over a named numeric property, and union
coverage) on the rich path (ADR-0018):

```ts
const analytics = ruleset.query(trucks, {
  spatial: { predicate: 'intersects' },
  aggregate: {
    count: true,
    min: 'speedLimit', max: 'speedLimit', avg: 'speedLimit',
    coverage: true,
  },
});
const perTruck = JSON.parse(analytics.toOutcomesJson());
// perTruck[0] = { outcome: 'matched', ruleIds: ['school-zone', 'downtown'],
//   aggregate: { count: 2, min: 30, max: 50, avg: 40, coverage: 0 } }  // point → coverage 0
// perTruck[1] = { outcome: 'matched', ruleIds: ['school-zone', 'downtown', 'no-delivery'],
//   aggregate: { count: 3, min: 30, max: 50, avg: 40, coverage: 0 } }  // no-delivery has no speedLimit
// perTruck[3] = { outcome: 'notMatched' }                              // no aggregate
```

Coverage is meaningful for polygon candidates — what fraction of a parcel the
applicable zones cover:

```ts
const parcel = {
  type: 'FeatureCollection',
  features: [
    { type: 'Feature', id: 'parcel', properties: {},
      geometry: { type: 'Polygon', coordinates: [[[4, 4], [4, 12], [12, 12], [12, 4], [4, 4]]] } },
  ],
};
const parcelCoverage = ruleset.query(parcel, {
  spatial: { predicate: 'intersects' },
  aggregate: { coverage: true },
}).toOutcomesJson();
// aggregate.coverage ≈ 0.94: the union of school-zone ∪ downtown covers ~94%
// of the parcel (planar estimate 60/64); coverage is the union, not a sum, so
// overlapping zones are never double-counted
```

## 8. Everything together

One query combining geofencing, temporal admission, property filters,
resolution, and aggregation:

```ts
const ops = ruleset.resolve(trucks, {
  spatial: { predicate: 'withinDistance', distance: 100 },
  at: '2026-08-24T10:00',
  where: {
    $and: [
      { $activeAt: { daysOfWeek: 'daysOfWeek', startHour: 'startHour', endHour: 'endHour' } },
      { kind: 'commercial' },
    ],
  },
  aggregate: { count: true, avg: 'taxRate', coverage: true },
});
const plan = JSON.parse(ops.toJson());
// per truck: { outcome, winner, values, applicable, aggregate } over the
// temporally- and distance-admitted commercial zones
```

## 9. Async

The same surfaces compute off the main thread:

```ts
const asyncMask = await ruleset.queryAsync(trucks, { spatial: { predicate: 'intersects' } });
asyncMask.count();

const asyncDecisions = await ruleset.resolveAsync(trucks, {
  spatial: { predicate: 'intersects' },
  aggregate: { count: true },
});
JSON.parse(asyncDecisions.toJson())[0].aggregate.count; // 2 (truck-1)
```

## 10. Hot reload and canonical round-trip

Rulesets are immutable and atomically replaceable; a failed replacement keeps
the old ruleset. The canonical form round-trips validated rules.

```ts
const report = ruleset.replace(replacementRules);   // { version, ruleCount, ... }
const snapshot = ruleset.toCanonical();             // validated rules as JSON
const reload = ruleset.replaceFromCanonical(Buffer.from(snapshot));
```

## Error handling

Construction and query errors throw `SpatialRulesError` with a stable `.code`:

```ts
import { SpatialRulesError } from 'spatial-rules';

try {
  ruleset.query(trucks, { spatial: { predicate: 'withinDistance' } }); // missing distance
} catch (err) {
  if (err instanceof SpatialRulesError && err.code === 'SR_INVALID_QUERY') {
    // the withinDistance predicate requires a positive 'distance'
  }
}
```