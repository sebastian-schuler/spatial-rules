// Performance comparison: turf.js (JS baseline A) vs the native addon.
//
// Same workload both sides: ~30 country-scale MultiPolygon rules × ~1,000
// footprint candidates, answering "does candidate intersect any rule".
//
//   node perf.mjs   (or: bun perf.mjs)
//
// Caveats (kept explicit):
//   - turf.js runs the naive 30×1,000 `booleanIntersects` (ladder A).
//   - the addon runs the real hot path: Buffer in → Uint8Array mask out
//     (includes GeoJSON re-parse per call, like the production path).

import { createRequire } from 'node:module';
import { readFileSync } from 'node:fs';
import { dirname, join } from 'node:path';
import { fileURLToPath } from 'node:url';
import { performance } from 'node:perf_hooks';
import { feature, booleanIntersects } from '@turf/turf';

const here = dirname(fileURLToPath(import.meta.url));
const require = createRequire(import.meta.url);
const { SpatialRuleset } = require(join(here, '..', '..', 'node', 'spatial_rules.node'));

const rules = JSON.parse(readFileSync(join(here, '..', 'data', 'rules.geojson'), 'utf8')).features;
const candidates = JSON.parse(readFileSync(join(here, '..', 'data', 'candidates.geojson'), 'utf8')).features;
const query = JSON.stringify({ spatial: { predicate: 'intersects' } });

const ruleFeatures = rules.map((rule) => feature(rule.geometry));
const candidateFeatures = candidates.map((candidate) => feature(candidate.geometry));

const candidatesBuffer = Buffer.from(readFileSync(join(here, '..', 'data', 'candidates.geojson')));
const ruleset = new SpatialRuleset(Buffer.from(readFileSync(join(here, '..', 'data', 'rules.geojson'))));

function turfBatch() {
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

function nativeBatch() {
  const mask = ruleset.query(candidatesBuffer, query);
  let matched = 0;
  for (const value of mask) if (value === 1) matched += 1;
  return matched;
}

function time(label, fn, iterations) {
  // Warmup (JIT, lazy init) — not timed.
  fn();
  const samples = [];
  for (let i = 0; i < iterations; i += 1) {
    const start = performance.now();
    const result = fn();
    const ms = performance.now() - start;
    samples.push(ms);
    console.log(`  ${label} run ${i + 1}: ${ms.toFixed(1)} ms (matches=${result})`);
  }
  const mean = samples.reduce((a, b) => a + b, 0) / samples.length;
  return { mean, min: Math.min(...samples) };
}

const iterations = Number(process.env.ITERS ?? 3);

console.log(`workload: ${rules.length} rules × ${candidates.length} candidates (intersects, batch)`);
console.log('turf.js (baseline A: naive, early-exit on first match):');
const turf = time('turf', turfBatch, iterations);
console.log('native addon (Buffer → Uint8Array mask):');
const native = time('native', nativeBatch, iterations);

console.log(`\nturf mean ${turf.mean.toFixed(1)} ms | native mean ${native.mean.toFixed(1)} ms | speedup ${(turf.mean / native.mean).toFixed(1)}×`);
