// Smoke test for the binding — runnable under both Node and Bun:
//   node test/smoke.ts
//   bun test/smoke.ts
//
// Build the addon first: cargo build -p spatial-rules-node
// and copy it to `node/spatial_rules.node` (see the ticket Answer).

import assert from 'node:assert/strict';
import { SpatialRulesError, SpatialRuleset } from '../index.ts';

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
        properties: { active: true },
        geometry: { type: 'Polygon', coordinates: [[[0, 0], [0, 10], [10, 10], [10, 0], [0, 0]]] },
      },
      {
        type: 'Feature',
        id: 'zone-b',
        properties: { active: false },
        geometry: { type: 'Polygon', coordinates: [[[100, 100], [100, 110], [110, 110], [110, 100], [100, 100]]] },
      },
      {
        type: 'Feature',
        id: 'zone-c',
        properties: { active: true },
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
const mask = result.toMask();
assert.ok(mask instanceof Uint8Array, 'mask is a Uint8Array');
assert.equal(mask.length, 3);
assert.deepEqual(Array.from(mask), [1, 0, 2]);

// Chainable result (filtering-scale ticket 03): one query() call, many views;
// everything except the rich view is derived in JS with no extra crossing.
assert.equal(result.count(), 1);
assert.deepEqual(Array.from(result.toIndices()), [0]); // only "inside" matched
assert.deepEqual(result.summary(), { matched: 1, notMatched: 1, invalid: 1 });
assert.deepEqual(Array.from(result.invalidIndices()), [2]); // the bowtie
const kept = JSON.parse(result.toGeoJson());
assert.equal(kept.type, 'FeatureCollection');
assert.equal(kept.features.length, 1);
assert.equal(kept.features[0].id, 'inside');
assert.equal(kept.features[0].properties.name, 'inside-poly'); // properties preserved
const richResult = JSON.parse(result.toRichJson());
assert.equal(richResult[0].outcome, 'matched');
assert.deepEqual(richResult[0].ruleIds, ['zone-a', 'zone-c']);
// The rich view is cached: a second call returns the identical string.
assert.equal(result.toRichJson(), result.toRichJson());

// Property `where` filters out zone-b.
const active = ruleset.query(candidates, JSON.stringify({ spatial: { predicate: 'intersects' }, where: { active: true } })).toMask();
assert.deepEqual(Array.from(active), [1, 0, 2]);

// Richer where operators (ADR-0011): $exists / $nin / $not pass through.
const exists = ruleset.query(candidates, JSON.stringify({ spatial: { predicate: 'intersects' }, where: { active: { $exists: true } } })).toMask();
assert.deepEqual(Array.from(exists), [1, 0, 2]);

const nin = ruleset.query(candidates, JSON.stringify({ spatial: { predicate: 'intersects' }, where: { active: { $nin: [true] } } })).toMask();
assert.deepEqual(Array.from(nin), [0, 0, 2]);

const not = ruleset.query(candidates, JSON.stringify({ spatial: { predicate: 'intersects' }, where: { active: { $not: { $eq: false } } } })).toMask();
assert.deepEqual(Array.from(not), [1, 0, 2]);

// Whole-clause $nor (filtering-scale ticket 02): NOT(active = true) keeps only
// the inactive zone-b, which no candidate reaches.
const nor = ruleset.query(candidates, JSON.stringify({ spatial: { predicate: 'intersects' }, where: { $nor: [{ active: true }] } })).toMask();
assert.deepEqual(Array.from(nor), [0, 0, 2]);

// excludeRuleIds removes named rules: excluding both matching zones leaves none.
const excluding = ruleset.query(candidates, JSON.stringify({ spatial: { predicate: 'intersects' }, excludeRuleIds: ['zone-a', 'zone-c'] })).toMask();
assert.deepEqual(Array.from(excluding), [0, 0, 2]);

// Rich per-candidate outcomes with original string rule ids.
const rich = JSON.parse(ruleset.queryRich(candidates, intersects));
assert.equal(rich.length, 3);
assert.equal(rich[0].outcome, 'matched');
// The first candidate intersects both zone-a and zone-c (multi-match).
assert.deepEqual(rich[0].ruleIds, ['zone-a', 'zone-c']);
assert.equal(rich[1].outcome, 'notMatched');
assert.equal(rich[2].outcome, 'invalid');
// No overlap payload unless requested.
assert.ok(!('overlaps' in rich[0]));

// queryRich honors includeOverlap (ADR-0012): matched outcomes carry per-rule
// geodesic overlapArea/overlapRatio.
const richOverlap = JSON.parse(ruleset.queryRich(candidates, JSON.stringify({ spatial: { predicate: 'intersects' }, includeOverlap: true })));
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
const coveredBy = ruleset.query(candidates, JSON.stringify({ spatial: { predicate: 'covered_by' } })).toMask();
assert.deepEqual(Array.from(coveredBy), [1, 0, 2]);
const covers = ruleset.query(candidates, JSON.stringify({ spatial: { predicate: 'covers' } })).toMask();
assert.deepEqual(Array.from(covers), [0, 0, 2]);
const touches = ruleset.query(candidates, JSON.stringify({ spatial: { predicate: 'touches' } })).toMask();
assert.deepEqual(Array.from(touches), [0, 0, 2]);
const overlaps = ruleset.query(candidates, JSON.stringify({ spatial: { predicate: 'overlaps' } })).toMask();
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
assert.deepEqual(Array.from(ruleset.query(pointCandidates, intersects).toMask()), [1, 0]);

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
assert.deepEqual(Array.from(ruleset.query(candidates, intersects).toMask()), [0, 0, 2]);

// Canonical ruleset persistence (ADR-0013): toJSON / fromCanonical round-trip.
const canonical = ruleset.toJSON();
const parsedCanonical = JSON.parse(canonical);
assert.ok(Array.isArray(parsedCanonical));
assert.equal(parsedCanonical.length, 1);
assert.equal(parsedCanonical[0].id, 'zone-d');

const canonicalReport = JSON.parse(ruleset.fromCanonical(Buffer.from(canonical)));
assert.equal(canonicalReport.version, 3);
assert.equal(canonicalReport.ruleCount, 1);

// Invalid canonical input rejects and leaves the ruleset untouched.
assert.throws(
  () => ruleset.fromCanonical(Buffer.from('not json')),
  (e) => e instanceof SpatialRulesError && e.code === 'SR_INVALID_GEOJSON',
);
assert.equal(JSON.parse(ruleset.stats()).version, 3);

// Opt-in async query (ADR-0009 amendment): same mask as the sync path.
const asyncMask = await ruleset.queryAsync(candidates, intersects);
assert.deepEqual(Array.from(asyncMask), Array.from(ruleset.query(candidates, intersects).toMask()));

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
const preMask = Array.from(ruleset.query(candidates, intersects).toMask()); // [0,0,2]
const inFlight = ruleset.queryAsync(candidates, intersects);
ruleset.replace(replacement2); // "inside" now matches -> [1,0,2]
const settled = Array.from(await inFlight);
const postMask = Array.from(ruleset.query(candidates, intersects).toMask());
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
  Array.from(dynamicRuleset.query(candidatesObject, intersectsObject).toMask()),
  [1, 0, 2],
);
// String candidates with a string query (Buffer forms already covered above).
assert.deepEqual(
  Array.from(dynamicRuleset.query(candidatesString, intersects).toMask()),
  [1, 0, 2],
);
// Constructor accepts a string; Buffer candidates with an object query.
const stringRuleset = new SpatialRuleset(rulesString);
assert.deepEqual(
  Array.from(stringRuleset.query(candidates, intersectsObject).toMask()),
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

// SpatialRulesError is a real Error subclass carrying a stable code (ADR-0005).
const directError = new SpatialRulesError('boom', 'SR_TEST');
assert.ok(directError instanceof Error);
assert.equal(directError.name, 'SpatialRulesError');
assert.equal(directError.code, 'SR_TEST');
assert.equal(directError.message, 'boom');

// Empty candidate batch: every terminal derives from an empty mask.
const emptyCandidates = Buffer.from(JSON.stringify({ type: 'FeatureCollection', features: [] }));
const emptyResult = ruleset.query(emptyCandidates, intersects);
assert.deepEqual(Array.from(emptyResult.toMask()), []);
assert.deepEqual(Array.from(emptyResult.toIndices()), []);
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
assert.deepEqual(Array.from(soloResult.toMask()), [1]);
const soloGeojson = JSON.parse(soloResult.toGeoJson());
assert.equal(soloGeojson.type, 'FeatureCollection');
assert.equal(soloGeojson.features.length, 1);
assert.equal(soloGeojson.features[0].id, 'solo');
assert.equal(soloGeojson.features[0].properties.name, 'solo');

console.log('smoke test passed');
