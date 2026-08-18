// Clean-install smoke — verifies the zero-toolchain install path (ADR-0006,
// ticket 18): a fresh directory with only the packed `spatial-rules` +
// per-platform tarballs installed, no Rust, no repo checkout. The loader must
// resolve the installed `spatial-rules-<triple>` optionalDependency package
// rather than a local build.
//
//   # in a temp project dir (see ticket 18 comment for the full loop):
//   npm install /path/to/spatial-rules-0.1.0.tgz /path/to/spatial-rules-win32-x64-msvc-0.1.0.tgz
//   node clean-install.mjs      # copy this file there first (bare 'spatial-rules'
//                               # resolves from THIS file's location, so it must
//                               # live inside the temp project, not the repo)
//
// Mirrors node/test/smoke.mjs but against the installed package.

import assert from 'node:assert/strict';
import { SpatialRuleset } from 'spatial-rules';

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

const mask = ruleset.query(candidates, intersects);
assert.ok(mask instanceof Uint8Array);
assert.deepEqual(Array.from(mask), [1, 0, 2]);

// Property `where` filters out zone-b.
const active = ruleset.query(candidates, JSON.stringify({ spatial: { predicate: 'intersects' }, where: { active: true } }));
assert.deepEqual(Array.from(active), [1, 0, 2]);

// excludeRuleIds removes named rules: excluding zone-a leaves none matched.
const excluding = ruleset.query(candidates, JSON.stringify({ spatial: { predicate: 'intersects' }, excludeRuleIds: ['zone-a'] }));
assert.deepEqual(Array.from(excluding), [0, 0, 2]);

// Rich per-candidate outcomes with original string rule ids.
const rich = JSON.parse(ruleset.queryRich(candidates, intersects));
assert.deepEqual(rich[0].ruleIds, ['zone-a']);
assert.equal(rich[1].outcome, 'notMatched');
assert.equal(rich[2].outcome, 'invalid');

// replace + stats (Engine path) are present on the installed package.
const report = JSON.parse(ruleset.replace(rules));
assert.equal(report.ruleCount, 2);
assert.equal(JSON.parse(ruleset.stats()).ruleCount, 2);

console.log('clean-install smoke passed (zero-toolchain install path works)');
