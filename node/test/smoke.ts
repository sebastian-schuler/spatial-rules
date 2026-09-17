// Smoke test for the binding — runnable under both Node and Bun:
//   node test/smoke.ts
//   bun test/smoke.ts
//
// Build the addon first: cargo build -p spatial-rules-node
// and copy it to `node/spatial_rules.node` (see the ticket Answer).

import assert from 'node:assert/strict';
import { ResolutionResult, SpatialRulesError, SpatialRuleset } from '../index.ts';

// Deliberately-invalid inputs: these assert the wrapper's runtime TypeErrors,
// so they are cast to `never` to bypass the compile-time signatures.
const invalid = (value: unknown) => value as never;

const rules = Buffer.from(
  JSON.stringify({
    type: 'FeatureCollection',
    features: [
      {
        type: 'Feature',
        id: 'zone-a',
        priority: 10,
        properties: { active: true, name: 'a', shared: 'from-a', priority: 999 },
        geometry: { type: 'Polygon', coordinates: [[[0, 0], [0, 10], [10, 10], [10, 0], [0, 0]]] },
      },
      {
        type: 'Feature',
        id: 'zone-b',
        priority: 5,
        properties: { active: false, name: 'b' },
        geometry: { type: 'Polygon', coordinates: [[[100, 100], [100, 110], [110, 110], [110, 100], [100, 100]]] },
      },
      {
        type: 'Feature',
        id: 'zone-c',
        priority: 20,
        properties: { active: true, name: 'c' },
        geometry: { type: 'Polygon', coordinates: [[[2, 2], [2, 12], [12, 12], [12, 2], [2, 2]]] },
      },
    ],
  }),
);

const candidates = Buffer.from(
  JSON.stringify({
    type: 'FeatureCollection',
    features: [
      // inside zone-a
      { type: 'Feature', id: 'inside', properties: { name: 'inside-poly' }, geometry: { type: 'Polygon', coordinates: [[[2, 2], [2, 4], [4, 4], [4, 2], [2, 2]]] } },
      // disjoint from both zones
      { type: 'Feature', id: 'far', properties: {}, geometry: { type: 'Polygon', coordinates: [[[50, 50], [50, 60], [60, 60], [60, 50], [50, 50]]] } },
      // invalid bowtie
      { type: 'Feature', id: 'invalid', properties: {}, geometry: { type: 'Polygon', coordinates: [[[0, 0], [10, 10], [0, 10], [10, 0], [0, 0]]] } },
    ],
  }),
);

const intersects = JSON.stringify({ spatial: { predicate: 'intersects' } });

const ruleset = new SpatialRuleset(rules);

// Hot path: Uint8Array mask (0 no match, 1 matched, 2 invalid), aligned to input.
const result = ruleset.query(candidates, intersects);
const mask = result.mask();
assert.ok(mask instanceof Uint8Array, 'mask is a Uint8Array');
assert.equal(mask.length, 3);
assert.deepEqual(Array.from(mask), [1, 0, 2]);

// Chainable result (filtering-scale ticket 03): one query() call, many views;
// everything except the outcomes view is derived in JS with no extra crossing.
assert.equal(result.count(), 1);
assert.deepEqual(Array.from(result.indices()), [0]); // only "inside" matched
assert.deepEqual(result.summary(), { matched: 1, notMatched: 1, invalid: 1 });
assert.deepEqual(Array.from(result.invalidIndices()), [2]); // the bowtie
const kept = JSON.parse(result.toGeoJson());
assert.equal(kept.type, 'FeatureCollection');
assert.equal(kept.features.length, 1);
assert.equal(kept.features[0].id, 'inside');
assert.equal(kept.features[0].properties.name, 'inside-poly'); // properties preserved
const richResult = JSON.parse(result.toOutcomesJson());
assert.equal(richResult[0].outcome, 'matched');
assert.deepEqual(richResult[0].ruleIds, ['zone-a', 'zone-c']);
// The outcomes view is cached: a second call returns the identical string.
assert.equal(result.toOutcomesJson(), result.toOutcomesJson());

// Property `where` filters out zone-b.
const active = ruleset.query(candidates, JSON.stringify({ spatial: { predicate: 'intersects' }, where: { active: true } })).mask();
assert.deepEqual(Array.from(active), [1, 0, 2]);

// Richer where operators (ADR-0011): $exists / $nin / $not pass through.
const exists = ruleset.query(candidates, JSON.stringify({ spatial: { predicate: 'intersects' }, where: { active: { $exists: true } } })).mask();
assert.deepEqual(Array.from(exists), [1, 0, 2]);

const nin = ruleset.query(candidates, JSON.stringify({ spatial: { predicate: 'intersects' }, where: { active: { $nin: [true] } } })).mask();
assert.deepEqual(Array.from(nin), [0, 0, 2]);

const not = ruleset.query(candidates, JSON.stringify({ spatial: { predicate: 'intersects' }, where: { active: { $not: { $eq: false } } } })).mask();
assert.deepEqual(Array.from(not), [1, 0, 2]);

// Whole-clause $nor (filtering-scale ticket 02): NOT(active = true) keeps only
// the inactive zone-b, which no candidate reaches.
const nor = ruleset.query(candidates, JSON.stringify({ spatial: { predicate: 'intersects' }, where: { $nor: [{ active: true }] } })).mask();
assert.deepEqual(Array.from(nor), [0, 0, 2]);

// excludeRuleIds removes named rules: excluding both matching zones leaves none.
const excluding = ruleset.query(candidates, JSON.stringify({ spatial: { predicate: 'intersects' }, excludeRuleIds: ['zone-a', 'zone-c'] })).mask();
assert.deepEqual(Array.from(excluding), [0, 0, 2]);

// Per-candidate outcomes with original string rule ids.
const rich = JSON.parse(ruleset.query(candidates, intersects).toOutcomesJson());
assert.equal(rich.length, 3);
assert.equal(rich[0].outcome, 'matched');
// The first candidate intersects both zone-a and zone-c (multi-match).
assert.deepEqual(rich[0].ruleIds, ['zone-a', 'zone-c']);
assert.equal(rich[1].outcome, 'notMatched');
assert.equal(rich[2].outcome, 'invalid');
// No overlap payload unless requested.
assert.ok(!('overlaps' in rich[0]));

// toOutcomesJson honors includeOverlap (ADR-0012): matched outcomes carry per-rule
// geodesic overlapArea/overlapRatio.
const richOverlap = JSON.parse(ruleset.query(candidates, JSON.stringify({ spatial: { predicate: 'intersects' }, includeOverlap: true })).toOutcomesJson());
assert.equal(richOverlap[0].outcome, 'matched');
assert.deepEqual(richOverlap[0].ruleIds, ['zone-a', 'zone-c']);
assert.equal(richOverlap[0].overlaps.length, 2);
for (const o of richOverlap[0].overlaps) {
  assert.equal(typeof o.overlapArea, 'number');
  assert.equal(typeof o.overlapRatio, 'number');
  assert.ok(o.overlapArea > 0);
  assert.ok(o.overlapRatio > 0 && o.overlapRatio <= 1);
}
assert.ok(!('overlaps' in richOverlap[1]));

// Structured errors: SpatialRulesError with stable SR_* codes (ADR-0005).
assert.throws(
  () => new SpatialRuleset(Buffer.from('not json')),
  (e) => e instanceof SpatialRulesError && e.code === 'SR_INVALID_GEOJSON',
);
// A negative top-level priority fails construction (ADR-0015): it would sort
// below unprioritized (0) rules, so it is rejected like a wrong type.
assert.throws(
  () => new SpatialRuleset(Buffer.from(JSON.stringify({
    type: 'FeatureCollection',
    features: [{ type: 'Feature', id: 'bad', priority: -5, properties: {}, geometry: { type: 'Polygon', coordinates: [[[0, 0], [0, 1], [1, 1], [1, 0], [0, 0]]] } }],
  }))),
  (e) => e instanceof SpatialRulesError && e.code === 'SR_RULESET_CONSTRUCTION_FAILED',
);
// Invalid UTF-8 buffers hit the same unified SR_* path.
assert.throws(
  () => ruleset.query(Buffer.from([0xff, 0xfe, 0x00]), intersects),
  (e) => e instanceof SpatialRulesError && e.code === 'SR_INVALID_GEOJSON',
);
assert.throws(
  () => ruleset.query(candidates, 'not json'),
  (e) => e instanceof SpatialRulesError && e.code === 'SR_INVALID_QUERY',
);
assert.throws(
  () => ruleset.query(candidates, JSON.stringify({ spatial: { predicate: 'crosses' } })),
  (e) => e instanceof SpatialRulesError && e.code === 'SR_UNSUPPORTED_SPATIAL_PREDICATE',
);

// Additional DE-9IM predicates (ADR-0012) pass through the same mask path.
const coveredBy = ruleset.query(candidates, JSON.stringify({ spatial: { predicate: 'covered_by' } })).mask();
assert.deepEqual(Array.from(coveredBy), [1, 0, 2]);
const covers = ruleset.query(candidates, JSON.stringify({ spatial: { predicate: 'covers' } })).mask();
assert.deepEqual(Array.from(covers), [0, 0, 2]);
const touches = ruleset.query(candidates, JSON.stringify({ spatial: { predicate: 'touches' } })).mask();
assert.deepEqual(Array.from(touches), [0, 0, 2]);
const overlaps = ruleset.query(candidates, JSON.stringify({ spatial: { predicate: 'overlaps' } })).mask();
assert.deepEqual(Array.from(overlaps), [0, 0, 2]);

// Point candidates (filtering-scale ticket 01): a point inside zone-a matches,
// a disjoint point does not.
const pointCandidates = Buffer.from(
  JSON.stringify({
    type: 'FeatureCollection',
    features: [
      { type: 'Feature', id: 'pt-in', properties: {}, geometry: { type: 'Point', coordinates: [5, 5] } },
      { type: 'Feature', id: 'pt-out', properties: {}, geometry: { type: 'Point', coordinates: [50, 50] } },
    ],
  }),
);
assert.deepEqual(Array.from(ruleset.query(pointCandidates, intersects).mask()), [1, 0]);

// withinDistance (P2, ADR-0016): a metric predicate passing through the same
// query surface. pt-in (inside zone-a) is within 100 m of it; pt-out is not.
const within = JSON.stringify({ spatial: { predicate: 'withinDistance', distance: 100 } });
assert.deepEqual(Array.from(ruleset.query(pointCandidates, within).mask()), [1, 0]);
// Strict validation: distance is required for withinDistance.
assert.throws(
  () => ruleset.query(pointCandidates, JSON.stringify({ spatial: { predicate: 'withinDistance' } })),
  (e) => e instanceof SpatialRulesError && e.code === 'SR_INVALID_QUERY',
);

// Aggregation (ADR-0018): per-candidate analytics over the applicable set,
// carried in the rich JSON. "inside" matches zone-a and zone-c; the union
// covers it fully; "far"/"invalid" get no aggregate.
const agg = JSON.parse(ruleset.query(candidates, {
  spatial: { predicate: 'intersects' },
  aggregate: { count: true, coverage: true },
}).toOutcomesJson());
assert.equal(agg[0].outcome, 'matched');
assert.equal(agg[0].aggregate.count, 2);
assert.ok(agg[0].aggregate.coverage > 0.9, `coverage ${agg[0].aggregate.coverage}`);
assert.ok(!('aggregate' in agg[1]), 'notMatched has no aggregate');
assert.ok(!('aggregate' in agg[2]), 'invalid has no aggregate');
// The aggregate rides the resolution rich path too, and async parity holds.
const resAgg = JSON.parse(ruleset.resolve(candidates, {
  spatial: { predicate: 'intersects' },
  aggregate: { count: true },
}).toJson());
assert.equal(resAgg[0].outcome, 'resolved');
assert.equal(resAgg[0].aggregate.count, 2);
assert.ok(!('aggregate' in resAgg[1]));
const aggAsync = await ruleset.queryAsync(candidates, {
  spatial: { predicate: 'intersects' },
  aggregate: { count: true },
});
assert.equal(JSON.parse(aggAsync.toOutcomesJson())[0].aggregate.count, 2);
// Strict validation: an unknown aggregate function is rejected.
assert.throws(
  () => ruleset.query(candidates, { spatial: { predicate: 'intersects' }, aggregate: { median: true } }),
  (e) => e instanceof SpatialRulesError && e.code === 'SR_INVALID_QUERY',
);

// Resolution (ticket 04, ADR-0015): resolve() returns a chainable
// ResolutionResult — a compact mask (0 no resolution, 1 resolved, 2 invalid)
// plus lazy rich toJson() with the winner, first-provider-wins values, and the
// ordered applicable set.
const resolution = ruleset.resolve(candidates, intersects);
assert.ok(resolution instanceof ResolutionResult, 'resolve() returns a ResolutionResult');
assert.deepEqual(Array.from(resolution.mask()), [1, 0, 2]);
assert.equal(resolution.count(), 1);
assert.deepEqual(resolution.summary(), { resolved: 1, notResolved: 1, invalid: 1 });

// toJson(): per-candidate {outcome, winner, values, applicable}. "inside"
// intersects zone-a (priority 10) and zone-c (priority 20): winner is zone-c;
// "shared" is gap-filled from zone-a because zone-c does not define it. The
// zone-a properties.priority (999) is plain metadata — merged as an ordinary
// property value, but never read for precedence (the winner is zone-c by its
// top-level priority 20).
const resolvedJson = JSON.parse(resolution.toJson());
assert.equal(resolvedJson.length, 3);
assert.equal(resolvedJson[0].outcome, 'resolved');
assert.equal(resolvedJson[0].winner, 'zone-c');
assert.deepEqual(resolvedJson[0].values, {
  active: true,
  name: 'c',
  priority: 999,
  shared: 'from-a',
});
assert.deepEqual(resolvedJson[0].applicable, [
  { ruleId: 'zone-c', priority: 20, spatialMatched: true, propertyMatched: true },
  { ruleId: 'zone-a', priority: 10, spatialMatched: true, propertyMatched: true },
]);
assert.equal(resolvedJson[1].outcome, 'notMatched');
assert.equal(resolvedJson[2].outcome, 'invalid');
assert.equal(typeof resolvedJson[2].reason, 'string');
// The rich view is cached: a second call returns the identical string.
assert.equal(resolution.toJson(), resolution.toJson());

// Resolution honors the same query shape as query(): where + excludeRuleIds.
const inactiveResolve = ruleset
  .resolve(candidates, JSON.stringify({ spatial: { predicate: 'intersects' }, where: { active: false } }))
  .mask();
assert.deepEqual(Array.from(inactiveResolve), [0, 0, 2]);
const excludedResolve = ruleset
  .resolve(candidates, JSON.stringify({ spatial: { predicate: 'intersects' }, excludeRuleIds: ['zone-a', 'zone-c'] }))
  .mask();
assert.deepEqual(Array.from(excludedResolve), [0, 0, 2]);

// resolveAsync(): same mask as resolve(), returned as a ResolutionResult.
const asyncResolution = await ruleset.resolveAsync(candidates, intersects);
assert.deepEqual(
  Array.from(asyncResolution.mask()),
  Array.from(ruleset.resolve(candidates, intersects).mask()),
);
assert.deepEqual(asyncResolution.summary(), resolution.summary());

// resolveAsync rejects with the same SR_* error model as queryAsync.
await assert.rejects(
  ruleset.resolveAsync(candidates, 'not json'),
  (e) => e instanceof SpatialRulesError && e.code === 'SR_INVALID_QUERY',
);

// Dynamic replacement (ADR-0007): atomic swap + observability.
const stats = JSON.parse(ruleset.stats());
assert.equal(stats.version, 1);
assert.equal(stats.ruleCount, 3);

const replacement = Buffer.from(
  JSON.stringify({
    type: 'FeatureCollection',
    features: [
      {
        type: 'Feature',
        id: 'zone-d',
        properties: {},
        geometry: { type: 'Polygon', coordinates: [[[100, 100], [100, 110], [110, 110], [110, 100], [100, 100]]] },
      },
    ],
  }),
);
const report = JSON.parse(ruleset.replace(replacement));
assert.equal(report.version, 2);
assert.equal(report.ruleCount, 1);

// After replacement the new ruleset is active: no candidate matches spatially.
assert.deepEqual(Array.from(ruleset.query(candidates, intersects).mask()), [0, 0, 2]);

// Canonical ruleset persistence (ADR-0013): toCanonical / replaceFromCanonical round-trip.
const canonical = ruleset.toCanonical();
const parsedCanonical = JSON.parse(canonical);
assert.ok(Array.isArray(parsedCanonical));
assert.equal(parsedCanonical.length, 1);
assert.equal(parsedCanonical[0].id, 'zone-d');

const canonicalReport = JSON.parse(ruleset.replaceFromCanonical(Buffer.from(canonical)));
assert.equal(canonicalReport.version, 3);
assert.equal(canonicalReport.ruleCount, 1);

// Invalid canonical input rejects and leaves the ruleset untouched.
assert.throws(
  () => ruleset.replaceFromCanonical(Buffer.from('not json')),
  (e) => e instanceof SpatialRulesError && e.code === 'SR_INVALID_GEOJSON',
);
assert.equal(JSON.parse(ruleset.stats()).version, 3);

// Opt-in async query (ADR-0009 amendment): same mask as the sync path,
// returned as a QueryResult so the chainable terminals work on it.
const asyncResult = await ruleset.queryAsync(candidates, intersects);
assert.deepEqual(
  Array.from(asyncResult.mask()),
  Array.from(ruleset.query(candidates, intersects).mask()),
);

// Same SR_* error model, surfaced as a Promise rejection.
await assert.rejects(
  ruleset.queryAsync(candidates, 'not json'),
  (e) => e instanceof SpatialRulesError && e.code === 'SR_INVALID_QUERY',
);
await assert.rejects(
  ruleset.queryAsync(Buffer.from([0xff, 0xfe, 0x00]), intersects),
  (e) => e instanceof SpatialRulesError && e.code === 'SR_INVALID_GEOJSON',
);

// An in-flight async query across a replace() observes one consistent
// snapshot (ADR-0007): the pre-replace or post-replace mask, never a torn mix.
const replacement2 = Buffer.from(
  JSON.stringify({
    type: 'FeatureCollection',
    features: [
      {
        type: 'Feature',
        id: 'zone-e',
        properties: {},
        geometry: { type: 'Polygon', coordinates: [[[0, 0], [0, 10], [10, 10], [10, 0], [0, 0]]] },
      },
    ],
  }),
);
const preMask = Array.from(ruleset.query(candidates, intersects).mask()); // [0,0,2]
const inFlight = ruleset.queryAsync(candidates, intersects);
ruleset.replace(replacement2); // "inside" now matches -> [1,0,2]
const settled = Array.from((await inFlight).mask());
const postMask = Array.from(ruleset.query(candidates, intersects).mask());
const matchesPre = settled.every((v, i) => v === preMask[i]);
const matchesPost = settled.every((v, i) => v === postMask[i]);
assert.ok(
  matchesPre || matchesPost,
  'in-flight queryAsync must observe a consistent snapshot',
);

// Dynamic input types (filtering-scale ticket 05): the wrapper normalizes
// Buffer | string | object for candidates + rules, and string | object for the
// query, before the native crossing. Each accepted type produces the same mask
// as the Buffer form; unsupported types throw a TypeError from the wrapper.
const candidatesObject = JSON.parse(candidates.toString('utf8'));
const candidatesString = candidates.toString('utf8');
const rulesObject = JSON.parse(rules.toString('utf8'));
const rulesString = rules.toString('utf8');
const intersectsObject = JSON.parse(intersects);

// Constructor accepts an object; query accepts object candidates + object query.
const dynamicRuleset = new SpatialRuleset(rulesObject);
assert.deepEqual(
  Array.from(dynamicRuleset.query(candidatesObject, intersectsObject).mask()),
  [1, 0, 2],
);
// String candidates with a string query (Buffer forms already covered above).
assert.deepEqual(
  Array.from(dynamicRuleset.query(candidatesString, intersects).mask()),
  [1, 0, 2],
);
// Constructor accepts a string; Buffer candidates with an object query.
const stringRuleset = new SpatialRuleset(rulesString);
assert.deepEqual(
  Array.from(stringRuleset.query(candidates, intersectsObject).mask()),
  [1, 0, 2],
);

// The result holds the normalized Buffer, so toGeoJson stays value-faithful
// for object inputs (properties preserved, formatting normalized).
const objectResult = dynamicRuleset.query(candidatesObject, intersectsObject);
const objectKept = JSON.parse(objectResult.toGeoJson());
assert.equal(objectKept.features.length, 1);
assert.equal(objectKept.features[0].id, 'inside');
assert.equal(objectKept.features[0].properties.name, 'inside-poly');

// replace() accepts object and string rules.
assert.equal(JSON.parse(dynamicRuleset.replace(rulesObject)).version, 2);
assert.equal(JSON.parse(dynamicRuleset.replace(rulesString)).version, 3);

// Unsupported input types throw a clear TypeError from the wrapper.
assert.throws(() => new SpatialRuleset(invalid(42)), TypeError);
assert.throws(() => new SpatialRuleset(invalid(null)), TypeError);
assert.throws(() => new SpatialRuleset(invalid(undefined)), TypeError);
assert.throws(() => new SpatialRuleset(invalid([])), TypeError);
assert.throws(() => dynamicRuleset.query(invalid(42), intersects), TypeError);
assert.throws(() => dynamicRuleset.query(candidates, invalid(42)), TypeError);
assert.throws(() => dynamicRuleset.query(candidates, invalid(null)), TypeError);
assert.throws(() => dynamicRuleset.query(candidates, invalid([])), TypeError);
assert.throws(() => dynamicRuleset.replace(invalid(42)), TypeError);
assert.throws(() => dynamicRuleset.replace(invalid([])), TypeError);

// resolve() normalizes inputs identically to query() (object/string forms).
assert.deepEqual(
  Array.from(dynamicRuleset.resolve(candidatesObject, intersectsObject).mask()),
  [1, 0, 2],
);
assert.deepEqual(
  Array.from(dynamicRuleset.resolve(candidatesString, intersects).mask()),
  [1, 0, 2],
);
assert.throws(() => dynamicRuleset.resolve(invalid(42), intersects), TypeError);
assert.throws(() => dynamicRuleset.resolve(candidates, invalid(42)), TypeError);

// SpatialRulesError is a real Error subclass carrying a stable code (ADR-0005).
const directError = new SpatialRulesError('boom', 'SR_TEST');
assert.ok(directError instanceof Error);
assert.equal(directError.name, 'SpatialRulesError');
assert.equal(directError.code, 'SR_TEST');
assert.equal(directError.message, 'boom');

// Empty candidate batch: every terminal derives from an empty mask.
const emptyCandidates = Buffer.from(JSON.stringify({ type: 'FeatureCollection', features: [] }));
const emptyResult = ruleset.query(emptyCandidates, intersects);
assert.deepEqual(Array.from(emptyResult.mask()), []);
assert.deepEqual(Array.from(emptyResult.indices()), []);
assert.deepEqual(Array.from(emptyResult.invalidIndices()), []);
assert.equal(emptyResult.count(), 0);
assert.deepEqual(emptyResult.summary(), { matched: 0, notMatched: 0, invalid: 0 });
const emptyGeojson = JSON.parse(emptyResult.toGeoJson());
assert.equal(emptyGeojson.type, 'FeatureCollection');
assert.deepEqual(emptyGeojson.features, []);

// A single Feature (not wrapped in a FeatureCollection) is accepted and
// round-trips through toGeoJson (the wrapper's `[parsed]` branch).
const singleFeature = Buffer.from(JSON.stringify({
  type: 'Feature',
  id: 'solo',
  properties: { name: 'solo' },
  geometry: { type: 'Polygon', coordinates: [[[2, 2], [2, 4], [4, 4], [4, 2], [2, 2]]] },
}));
const soloResult = ruleset.query(singleFeature, intersects);
assert.deepEqual(Array.from(soloResult.mask()), [1]);
const soloGeojson = JSON.parse(soloResult.toGeoJson());
assert.equal(soloGeojson.type, 'FeatureCollection');
assert.equal(soloGeojson.features.length, 1);
assert.equal(soloGeojson.features[0].id, 'solo');
assert.equal(soloGeojson.features[0].properties.name, 'solo');

console.log('smoke test passed');
