// Crossover sweep — at how many candidates does the native binding beat a
// hand-rolled turf scan, on a real ruleset? Shows the addon's per-query floor
// (GeoJSON parse + napi + index + prepared relate) against turf's linear scan
// + bbox fast-reject, and reports the break-even candidate count.
//
//   bun crossover.mjs                                  # RULES_FILE (or synthetic fallback)
//   RULES_FILE=countries.geojson bun crossover.mjs     # real boundary rules
//   SIZES=20,200,1000,5000 REPS=5 bun crossover.mjs    # sweep config

import { readFileSync } from 'node:fs';
import { performance } from 'node:perf_hooks';
import { feature, booleanIntersects, bbox } from '@turf/turf';
import {
  loadNative,
  makeRng,
  makeRules,
  makeCandidates as makeGridCandidates,
  toCollection,
} from './common.mjs';

const { SpatialRuleset } = loadNative();

const RULES = Number(process.env.RULES ?? 500);
const SIZES = (process.env.SIZES ?? '20,200,1000,5000').split(',').map(Number);
const REPS = Number(process.env.REPS ?? 3);

// --- rules ---------------------------------------------------------------

function loadRules() {
  if (!process.env.RULES_FILE) return { features: makeRules(RULES), dropped: [] };
  const raw = readFileSync(process.env.RULES_FILE, 'utf8');
  const geo = JSON.parse(raw);
  const features = geo.type === 'FeatureCollection' ? geo.features : [geo];
  for (let i = 0; i < features.length; i += 1) {
    const f = features[i];
    if (f.id == null && f.properties?.id == null) {
      f.id = f.properties?.ne_id != null ? `ne-${f.properties.ne_id}` : `rule-${i}`;
    }
  }
  // The engine validates strictly (ADR-0005) and rejects the whole ruleset if
  // any rule is invalid; drop the ones it rejects so both sides agree.
  const valid = [];
  const dropped = [];
  for (const f of features) {
    try {
      new SpatialRuleset(Buffer.from(JSON.stringify(toCollection([f]))));
      valid.push(f);
    } catch (e) {
      if (e?.code === 'SR_INVALID_GEOMETRY') dropped.push(f.id);
      else throw e;
    }
  }
  return { features: valid, dropped };
}

// --- candidates ----------------------------------------------------------

function* exteriorRings(geometry) {
  if (!geometry) return;
  if (geometry.type === 'Polygon') {
    yield geometry.coordinates[0];
    return;
  }
  if (geometry.type === 'MultiPolygon') {
    for (const poly of geometry.coordinates) yield poly[0];
    return;
  }
  if (geometry.type === 'GeometryCollection') {
    for (const g of geometry.geometries) yield* exteriorRings(g);
  }
}

function ringPoints(features) {
  const pts = [];
  for (const f of features) {
    for (const ring of exteriorRings(f.geometry)) {
      for (let i = 0; i < ring.length - 1; i += 1) pts.push(ring[i]);
    }
  }
  return pts;
}

function square(x, y, w, id) {
  return {
    type: 'Feature',
    id,
    properties: {},
    geometry: {
      type: 'Polygon',
      coordinates: [
        [[x - w, y - w], [x - w, y + w], [x + w, y + w], [x + w, y - w], [x - w, y - w]],
      ],
    },
  };
}

// Real-data mode: evenly sample exterior-ring vertices across all rules, each
// a tiny square sized from the rules' bbox — every candidate overlaps at least
// its source rule, so the relate work is real. Synthetic mode: grid cells
// centred on blob rules (from common.mjs), one overlap per candidate.
function makeCandidates(count) {
  if (process.env.RULES_FILE) {
    const pts = ringPoints(ruleGeo);
    const [minX, minY, maxX, maxY] = bbox(toCollection(ruleGeo));
    const w = Math.max(maxX - minX, maxY - minY) * 0.0005;
    const out = [];
    for (let i = 0; i < count; i += 1) {
      const p = pts[Math.floor((i * pts.length) / count) % pts.length];
      out.push(square(p[0], p[1], w, `cand-${i}`));
    }
    return out;
  }
  return makeGridCandidates(count, ruleGeo.length, makeRng(0x51a7_0001));
}

// --- timing --------------------------------------------------------------

function bboxOverlap(a, b) {
  return a[0] <= b[2] && a[2] >= b[0] && a[1] <= b[3] && a[3] >= b[1];
}

function countMask(mask) {
  let n = 0;
  for (const v of mask) if (v === 1) n += 1;
  return n;
}

function turfScan(candidateFeatures, candidateBboxes) {
  let matched = 0;
  for (let c = 0; c < candidateFeatures.length; c += 1) {
    const cb = candidateBboxes[c];
    for (let i = 0; i < ruleFeatures.length; i += 1) {
      if (!bboxOverlap(cb, ruleBboxes[i])) continue;
      if (booleanIntersects(candidateFeatures[c], ruleFeatures[i])) {
        matched += 1;
        break;
      }
    }
  }
  return matched;
}

function minOf(fn, reps) {
  let best = Infinity;
  for (let r = 0; r < reps; r += 1) {
    const t0 = performance.now();
    fn();
    const ms = performance.now() - t0;
    if (ms < best) best = ms;
  }
  return best;
}

// --- main ----------------------------------------------------------------

const { features: ruleGeo, dropped = [] } = loadRules();
const ruleFeatures = ruleGeo.map((f) => feature(f.geometry));
const ruleBboxes = ruleGeo.map((f) => bbox(f));
const rulesBuffer = Buffer.from(JSON.stringify(toCollection(ruleGeo)));
const querySpatial = JSON.stringify({ spatial: { predicate: 'intersects' } });

const ruleset = new SpatialRuleset(rulesBuffer);
// Warm the per-thread prepared-geometry cache (ADR-0010) — one-time, not timed.
ruleset.query(Buffer.from(JSON.stringify(toCollection(makeCandidates(8)))), querySpatial);

console.log('crossover — native full query vs turf scan + bbox reject');
console.log(
  `mode=${process.env.RULES_FILE ? 'real-data' : 'synthetic'} rules=${ruleGeo.length}` +
    `${dropped.length ? ` (${dropped.length} invalid dropped)` : ''} sizes=${SIZES.join(',')} reps=${REPS}`,
);
console.log('');
console.log(' candidates  addon (ms)   turf (ms)   speedup   matched');

let firstWin = null;
for (const n of SIZES) {
  const cand = makeCandidates(n);
  const candFeatures = cand.map((f) => feature(f.geometry));
  const candBboxes = candFeatures.map((f) => bbox(f));
  const candBuffer = Buffer.from(JSON.stringify(toCollection(cand)));

  const nativeMatched = countMask(ruleset.query(candBuffer, querySpatial));
  const turfMatched = turfScan(candFeatures, candBboxes);
  if (nativeMatched !== turfMatched) {
    console.error(`mismatch at ${n} candidates: native=${nativeMatched} turf=${turfMatched}`);
    process.exit(1);
  }

  const nativeMs = minOf(() => ruleset.query(candBuffer, querySpatial), REPS);
  const turfMs = minOf(() => turfScan(candFeatures, candBboxes), REPS);
  const speedup = turfMs / nativeMs;
  if (firstWin === null && speedup > 1) firstWin = n;

  const speed = speedup >= 100 ? `${speedup.toFixed(0)}x` : `${speedup.toFixed(1)}x`;
  console.log(
    String(n).padStart(10) +
      nativeMs.toFixed(2).padStart(12) +
      turfMs.toFixed(2).padStart(11) +
      speed.padStart(9) +
      String(nativeMatched).padStart(9),
  );
}

console.log('');
if (firstWin === null) {
  console.log('break-even: turf is faster at every size tested (addon floor not yet amortized)');
} else if (firstWin === SIZES[0]) {
  console.log(`break-even: the addon wins from the smallest size tested (${firstWin}); its per-query floor only matters below that`);
} else {
  console.log(`break-even: the addon wins from ~${firstWin} candidates; below that its per-query floor dominates`);
}
