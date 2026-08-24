// Smoke test for the wasm build — runnable under Node and Deno:
//   node --experimental-strip-types --experimental-wasm-modules test/smoke.ts
//   deno run test/smoke.ts
//
// Build the wasm first: wasm-pack build --release --target bundler (in wasm/).
// Mirrors the node/integration smokes: the controlled-ruleset literals plus
// the production `~1k×30` matched count (481).

import assert from 'node:assert/strict';
import { readFileSync } from 'node:fs';
import { join } from 'node:path';
import { fileURLToPath } from 'node:url';
import { SpatialRulesError, SpatialRuleset } from '../index.ts';

const repoRoot = fileURLToPath(new URL('../../', import.meta.url));

const rules = {
  type: 'FeatureCollection',
  features: [
    {
      type: 'Feature',
      id: 'zone-a',
      priority: 10,
      properties: { active: true, name: 'a', shared: 'from-a', priority: 999, daysOfWeek: 1, startHour: 0, endHour: 24 },
      geometry: { type: 'Polygon', coordinates: [[[0, 0], [0, 10], [10, 10], [10, 0], [0, 0]]] },
    },
    {
      type: 'Feature',
      id: 'zone-b',
      priority: 5,
      properties: { active: false, name: 'b', daysOfWeek: 2, startHour: 0, endHour: 24 },
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
};

const candidates = {
  type: 'FeatureCollection',
  features: [
    { type: 'Feature', id: 'inside', properties: {}, geometry: { type: 'Polygon', coordinates: [[[2, 2], [2, 4], [4, 4], [4, 2], [2, 2]]] } },
    { type: 'Feature', id: 'far', properties: {}, geometry: { type: 'Polygon', coordinates: [[[50, 50], [50, 60], [60, 60], [60, 50], [50, 50]]] } },
    { type: 'Feature', id: 'invalid', properties: {}, geometry: { type: 'Polygon', coordinates: [[[0, 0], [10, 10], [0, 10], [10, 0], [0, 0]]] } },
  ],
};

const pointPair = {
  type: 'FeatureCollection',
  features: [
    { type: 'Feature', id: 'pt-in', properties: {}, geometry: { type: 'Point', coordinates: [5, 5] } },
    { type: 'Feature', id: 'pt-out', properties: {}, geometry: { type: 'Point', coordinates: [50, 50] } },
  ],
};

const intersects = { spatial: { predicate: 'intersects' } };
const ruleset = new SpatialRuleset(rules);

// Hot path: the compact mask (0 no match, 1 matched, 2 invalid).
const mask = ruleset.query(candidates, intersects).mask();
assert.ok(mask instanceof Uint8Array, 'mask is a Uint8Array');
assert.deepEqual(Array.from(mask), [1, 0, 2]);

// Chainable result: count/summary/indices derive from the mask.
const result = ruleset.query(candidates, intersects);
assert.equal(result.count(), 1);
assert.deepEqual(result.summary(), { matched: 1, notMatched: 1, invalid: 1 });
assert.deepEqual(Array.from(result.indices()), [0]);
assert.deepEqual(Array.from(result.invalidIndices()), [2]);
assert.equal(JSON.parse(result.toGeoJson()).features.length, 1);

// Rich outcomes: string rule ids; "inside" reaches zone-a and zone-c.
const rich = JSON.parse(result.toOutcomesJson());
assert.equal(rich[0].outcome, 'matched');
assert.deepEqual(rich[0].ruleIds, ['zone-a', 'zone-c']);
assert.equal(rich[1].outcome, 'notMatched');
assert.equal(rich[2].outcome, 'invalid');

// withinDistance (ADR-0016): pt-in is inside zone-a (distance 0); pt-out is
// ~5,000 km away — only pt-in admits at 100 m.
const within = ruleset.query(pointPair, { spatial: { predicate: 'withinDistance', distance: 100 } }).mask();
assert.deepEqual(Array.from(within), [1, 0]);

// Temporal $activeAt (ADR-0017): zone-a is active Monday, zone-b Tuesday,
// zone-c has no window. Monday admits "inside"; Tuesday admits nothing it
// reaches.
const activeAt = { daysOfWeek: 'daysOfWeek', startHour: 'startHour', endHour: 'endHour' };
const monday = ruleset
  .query(candidates, { spatial: intersects.spatial, where: { $activeAt: activeAt }, at: '2026-08-24T10:00' })
  .mask();
assert.deepEqual(Array.from(monday), [1, 0, 2]);
const tuesday = ruleset
  .query(candidates, { spatial: intersects.spatial, where: { $activeAt: activeAt }, at: '2026-08-25T10:00' })
  .mask();
assert.deepEqual(Array.from(tuesday), [0, 0, 2]);

// Aggregation (ADR-0018) rides the rich query path: count 2 + full coverage.
const agg = JSON.parse(
  ruleset.query(candidates, { spatial: intersects.spatial, aggregate: { count: true, coverage: true } }).toOutcomesJson(),
);
assert.equal(agg[0].aggregate.count, 2);
assert.ok(agg[0].aggregate.coverage > 0.9, `coverage ${agg[0].aggregate.coverage}`);
assert.ok(!('aggregate' in agg[1]));

// Resolution (ADR-0015): compact mask + rich winner/values/applicable.
const resolution = ruleset.resolve(candidates, intersects);
assert.deepEqual(Array.from(resolution.mask()), [1, 0, 2]);
assert.equal(resolution.count(), 1);
assert.deepEqual(resolution.summary(), { resolved: 1, notResolved: 1, invalid: 1 });
const resolved = JSON.parse(resolution.toJson());
assert.equal(resolved[0].outcome, 'resolved');
assert.equal(resolved[0].winner, 'zone-c');
assert.deepEqual(resolved[0].values, {
  active: true,
  name: 'c',
  priority: 999,
  shared: 'from-a',
  daysOfWeek: 1,
  startHour: 0,
  endHour: 24,
});
assert.deepEqual(resolved[0].applicable, [
  { ruleId: 'zone-c', priority: 20, spatialMatched: true, propertyMatched: true },
  { ruleId: 'zone-a', priority: 10, spatialMatched: true, propertyMatched: true },
]);

// The aggregate rides the resolution rich path too.
const resolveAgg = JSON.parse(
  ruleset.resolve(candidates, { spatial: intersects.spatial, aggregate: { count: true } }).toJson(),
);
assert.equal(resolveAgg[0].aggregate.count, 2);
assert.ok(!('aggregate' in resolveAgg[1]));

// Canonical round-trip (ADR-0013).
const canonical = JSON.parse(ruleset.toCanonical());
assert.ok(Array.isArray(canonical));
assert.equal(canonical.length, 3);
assert.equal(canonical[0].id, 'zone-a');

// Structured errors carry the stable SR_* code.
assert.throws(
  () => ruleset.query(candidates, 'not json'),
  (e) => e instanceof SpatialRulesError && e.code === 'SR_INVALID_QUERY',
);
assert.throws(
  () => ruleset.query(candidates, { spatial: { predicate: 'crosses' } }),
  (e) => e instanceof SpatialRulesError && e.code === 'SR_UNSUPPORTED_SPATIAL_PREDICATE',
);

// Production workload: the ~1k×30 dataset, matched count pinned to the napi
// addon's 481.
const productionRules = JSON.parse(
  readFileSync(join(repoRoot, 'benchmarks/data/rules.geojson'), 'utf8'),
);
const productionCandidates = JSON.parse(
  readFileSync(join(repoRoot, 'benchmarks/data/candidates.geojson'), 'utf8'),
);
const production = new SpatialRuleset(productionRules).query(productionCandidates, intersects);
assert.equal(production.mask().length, productionCandidates.features.length);
assert.equal(production.count(), 481, `expected 481 matched, got ${production.count()}`);

console.log('wasm smoke passed');