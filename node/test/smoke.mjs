// Smoke test for the binding — runnable under both Node and Bun:
//   node test/smoke.mjs
//   bun test/smoke.mjs
//
// Build the addon first: cargo build -p spatial-rules-node
// and copy it to `node/spatial_rules.node` (see the ticket Answer).

import assert from 'node:assert/strict';
import { SpatialRulesError, SpatialRuleset } from '../index.js';

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
      { type: 'Feature', id: 'inside', properties: {}, geometry: { type: 'Polygon', coordinates: [[[2, 2], [2, 4], [4, 4], [4, 2], [2, 2]]] } },
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
const mask = ruleset.query(candidates, intersects);
assert.ok(mask instanceof Uint8Array, 'mask is a Uint8Array');
assert.equal(mask.length, 3);
assert.deepEqual(Array.from(mask), [1, 0, 2]);

// Property `where` filters out zone-b.
const active = ruleset.query(candidates, JSON.stringify({ spatial: { predicate: 'intersects' }, where: { active: true } }));
assert.deepEqual(Array.from(active), [1, 0, 2]);

// excludeRuleIds removes named rules: excluding both matching zones leaves none.
const excluding = ruleset.query(candidates, JSON.stringify({ spatial: { predicate: 'intersects' }, excludeRuleIds: ['zone-a', 'zone-c'] }));
assert.deepEqual(Array.from(excluding), [0, 0, 2]);

// Rich per-candidate outcomes with original string rule ids.
const rich = JSON.parse(ruleset.queryRich(candidates, intersects));
assert.equal(rich.length, 3);
assert.equal(rich[0].outcome, 'matched');
// The first candidate intersects both zone-a and zone-c (multi-match).
assert.deepEqual(rich[0].ruleIds, ['zone-a', 'zone-c']);
assert.equal(rich[1].outcome, 'notMatched');
assert.equal(rich[2].outcome, 'invalid');

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
  () => ruleset.query(candidates, JSON.stringify({ spatial: { predicate: 'overlaps' } })),
  (e) => e instanceof SpatialRulesError && e.code === 'SR_UNSUPPORTED_SPATIAL_PREDICATE',
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
assert.deepEqual(Array.from(ruleset.query(candidates, intersects)), [0, 0, 2]);

console.log('smoke test passed');
