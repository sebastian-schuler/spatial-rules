// "Fair" competitor — the best pure-JS answer (an rbush bbox index + turf
// relate) against the native addon, same full-mask output. The grid workload
// makes the bbox index genuinely selective (each candidate overlaps ~1 rule),
// so this is the strongest JS can do short of reimplementing geometry in a
// native addon — and the addon still wins.
//
//   node fair.mjs

import { performance } from 'node:perf_hooks';
import { feature, booleanIntersects, bbox } from '@turf/turf';
import RBush from 'rbush';
import {
  loadNative, matchedCount, makeRng, makeRules, makeCandidates, toCollection,
} from './common.mjs';

const { SpatialRuleset } = loadNative();

const RULES = Number(process.env.RULES ?? 300);
const CANDIDATES = Number(process.env.CANDIDATES ?? 1_000);

const rng = makeRng(0xf00d);
const ruleGeo = makeRules(RULES);
const candidateGeo = makeCandidates(CANDIDATES, RULES, rng);
const ruleFeatures = ruleGeo.map((f) => feature(f.geometry));
const candidateFeatures = candidateGeo.map((f) => feature(f.geometry));
const ruleset = new SpatialRuleset(Buffer.from(JSON.stringify(toCollection(ruleGeo))));
const candidatesBuffer = Buffer.from(JSON.stringify(toCollection(candidateGeo)));
const queryJson = JSON.stringify({ spatial: { predicate: 'intersects' } });

// rbush bbox index over the rules — the JS answer to "just index it".
const tree = new RBush(16);
tree.load(
  ruleFeatures.map((featureObj, index) => {
    const [minX, minY, maxX, maxY] = bbox(featureObj);
    return { minX, minY, maxX, maxY, index };
  }),
);

function naiveTurf() {
  let matched = 0;
  for (const candidate of candidateFeatures) {
    for (const rule of ruleFeatures) {
      if (booleanIntersects(candidate, rule)) {
        matched += 1;
        break;
      }
    }
  }
  return matched;
}

function indexedTurf() {
  let matched = 0;
  for (const candidate of candidateFeatures) {
    const [minX, minY, maxX, maxY] = bbox(candidate);
    for (const { index } of tree.search({ minX, minY, maxX, maxY })) {
      if (booleanIntersects(candidate, ruleFeatures[index])) {
        matched += 1;
        break;
      }
    }
  }
  return matched;
}

function nativeBatch() {
  return matchedCount(ruleset.query(candidatesBuffer, queryJson));
}

const expected = nativeBatch();
const naive = naiveTurf();
const indexed = indexedTurf();
if (naive !== expected || indexed !== expected) {
  console.error(`mismatch: naive=${naive} indexed=${indexed} native=${expected}`);
  process.exit(1);
}

function once(fn) {
  const start = performance.now();
  fn();
  return performance.now() - start;
}

console.log(`fair competitor — ${RULES} grid rules × ${CANDIDATES} candidates, ${expected} matched`);
console.log(`naive turf (scan)      : ${once(naiveTurf).toFixed(1)} ms`);
console.log(`rbush + turf (indexed) : ${once(indexedTurf).toFixed(1)} ms`);
console.log(`native addon           : ${once(nativeBatch).toFixed(2)} ms`);
