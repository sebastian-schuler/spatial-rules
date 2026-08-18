// Scaling sweep — turf.js vs the native addon as the workload grows.
//
// Demonstrates turf's O(candidates × rules) wall against the engine's batched,
// indexed, prepared-geometry query. The rules are spatially distributed on a
// grid, so the engine's bbox index filters to ~1 rule per candidate while a
// naive scan (turf) touches every rule.
//
//   node scale.mjs

import { performance } from 'node:perf_hooks';
import { feature, booleanIntersects } from '@turf/turf';
import {
  loadNative, matchedCount, makeRng, makeRules, makeCandidates, toCollection,
} from './common.mjs';

const { SpatialRuleset } = loadNative();

const points = [
  { rules: 30, candidates: 100 },
  { rules: 30, candidates: 1_000 },
  { rules: 30, candidates: 10_000 },
  { rules: 100, candidates: 1_000 },
  { rules: 300, candidates: 1_000 },
];

function turfAny(candidateFeatures, ruleFeatures) {
  let matched = 0;
  for (const candidate of candidateFeatures) {
    for (const rule of ruleFeatures) {
      if (booleanIntersects(candidate, rule)) {
        matched += 1;
        break; // turf's optimal strategy: stop at the first matching rule
      }
    }
  }
  return matched;
}

console.log('scaling sweep — turf.js (early-exit) vs native addon (full mask), intersects only');
console.log('rules  candidates  |  turf (ms)  |  addon (ms)  |  speedup');
for (const { rules: rn, candidates: cn } of points) {
  const rng = makeRng(0x5eed0000 ^ (rn * 7919 + cn));
  const ruleGeo = makeRules(rn);
  const candidateGeo = makeCandidates(cn, rn, rng);
  const ruleFeatures = ruleGeo.map((f) => feature(f.geometry));
  const candidateFeatures = candidateGeo.map((f) => feature(f.geometry));
  const ruleset = new SpatialRuleset(Buffer.from(JSON.stringify(toCollection(ruleGeo))));
  const candidatesBuffer = Buffer.from(JSON.stringify(toCollection(candidateGeo)));
  const queryJson = JSON.stringify({ spatial: { predicate: 'intersects' } });

  // Correctness: both sides must report the same matched count.
  const expected = matchedCount(ruleset.query(candidatesBuffer, queryJson));
  const actual = turfAny(candidateFeatures, ruleFeatures);
  if (expected !== actual) {
    console.error(`  ! mismatch at ${rn}×${cn}: turf=${actual} native=${expected}`);
    process.exit(1);
  }

  // Warmup only when the turf run is cheap (JIT); the large points are one-shot.
  if (rn * cn < 100_000) turfAny(candidateFeatures, ruleFeatures);

  const t0 = performance.now();
  turfAny(candidateFeatures, ruleFeatures);
  const turfMs = performance.now() - t0;

  const t1 = performance.now();
  ruleset.query(candidatesBuffer, queryJson);
  const nativeMs = performance.now() - t1;

  console.log(
    `${String(rn).padStart(5)}  ${String(cn).padStart(10)}  |  ${turfMs.toFixed(1).padStart(9)}  |  ${nativeMs.toFixed(2).padStart(10)}  |  ${(turfMs / nativeMs).toFixed(0)}×`,
  );
}
