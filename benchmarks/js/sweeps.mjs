// Sweep / experiment harnesses: scale, fair, complex, crossover.
//
//   bun sweeps.mjs scale [--points=30x100,100x200,300x1000] [--reps=3]
//   bun sweeps.mjs fair [--rules=300] [--candidates=1000] [--reps=3]
//   bun sweeps.mjs complex [--rules=3] [--parts=3] [--vertices=5000] [--fields=40]
//                          [--candidates=20] [--reps=3]
//                          [--rules-file=benchmarks/data/countries.geojson]
//   bun sweeps.mjs crossover [--mode=candidates|rules] [--rules=500]
//                            [--sizes=20,200,1000,5000] [--rules-range=500,1000,2000,5000]
//                            [--candidates=1000] [--reps=3]
//                            [--rules-file=benchmarks/data/countries.geojson]
//
// All knobs default to benchmarks.json; flags override. `--rules-file` switches
// to real-data mode (a GeoJSON boundary file such as Natural Earth; fetch with
// `bun run bench data`). Invoked from the repo root via `bun run bench <cmd>`.
// Every timed number is min-of-`reps` (one methodology, architecture-hardening
// 02); one-time setup costs (ruleset build, prepared-geometry warmup) are
// reported as single samples and labelled as such.

import { booleanIntersects, bbox } from '@turf/turf';
import RBush from 'rbush';
import {
  sectionConfig,
  resolveRepoPath,
  parsePoints,
  loadNative,
  matchedCount,
  makeRng,
  makeRules,
  makeGridCandidates,
  toCollection,
  loadRulesFromFile,
  ringPoints,
  exteriorRings,
  blobRing,
  square,
  timed,
  minOf,
  speedLabel,
  SPATIAL_QUERY,
} from './common.mjs';
import { toTurf, scanMatched } from './turf.mjs';

const { SpatialRuleset } = loadNative();

const sub = process.argv[2];
const args = process.argv.slice(3);

switch (sub) {
  case 'scale': runScale(args); break;
  case 'fair': runFair(args); break;
  case 'complex': runComplex(args); break;
  case 'crossover': runCrossover(args); break;
  default:
    console.error('usage: bun sweeps.mjs <scale|fair|complex|crossover> [flags]');
    process.exit(2);
}

// === scale — scaling sweep ================================================

function runScale(args) {
  const { section: scale } = sectionConfig('scale', args);
  const points = parsePoints(scale.points);
  const REPS = Number(scale.reps ?? 3);

  console.log('scaling sweep — turf.js (early-exit) vs native addon (full mask), intersects only');
  console.log('rules  candidates  |  turf (ms)  |  addon (ms)  |  speedup');
  for (const { rules: rn, candidates: cn } of points) {
    const rng = makeRng(0x5eed0000 ^ (rn * 7919 + cn));
    const ruleGeo = makeRules(rn);
    const candidateGeo = makeGridCandidates(cn, rn, rng);
    const ruleTurf = toTurf(ruleGeo);
    const candidateTurf = toTurf(candidateGeo);
    const ruleset = new SpatialRuleset(Buffer.from(JSON.stringify(toCollection(ruleGeo))));
    const candidatesBuffer = Buffer.from(JSON.stringify(toCollection(candidateGeo)));
    const queryJson = JSON.stringify(SPATIAL_QUERY);

    // Weakest baseline: naive scan, no index.
    const turf = () => scanMatched(candidateTurf, ruleTurf, { bbox: false });

    // Correctness: both sides must report the same matched count.
    const expected = matchedCount(ruleset.query(candidatesBuffer, queryJson));
    const actual = turf();
    if (expected !== actual) {
      console.error(`  ! mismatch at ${rn}×${cn}: turf=${actual} native=${expected}`);
      process.exit(1);
    }

    // Warmup only when the turf run is cheap (JIT); the large points are one-shot.
    if (rn * cn < 100_000) turf();

    const turfMs = minOf(turf, REPS);
    const nativeMs = minOf(() => ruleset.query(candidatesBuffer, queryJson), REPS);

    console.log(
      `${String(rn).padStart(5)}  ${String(cn).padStart(10)}  |  ${turfMs.toFixed(1).padStart(9)}  |  ${nativeMs.toFixed(2).padStart(10)}  |  ${(turfMs / nativeMs).toFixed(0)}×`,
    );
  }
}

// === fair — fair competitor (rbush + turf) ================================

function runFair(args) {
  const { section: fair } = sectionConfig('fair', args);
  const RULES = Number(fair.rules);
  const CANDIDATES = Number(fair.candidates);
  const REPS = Number(fair.reps ?? 3);

  const rng = makeRng(0xf00d);
  const ruleGeo = makeRules(RULES);
  const candidateGeo = makeGridCandidates(CANDIDATES, RULES, rng);
  const ruleTurf = toTurf(ruleGeo);
  const candidateTurf = toTurf(candidateGeo);
  const { features: ruleFeatures } = ruleTurf;
  const { features: candidateFeatures } = candidateTurf;
  const ruleset = new SpatialRuleset(Buffer.from(JSON.stringify(toCollection(ruleGeo))));
  const candidatesBuffer = Buffer.from(JSON.stringify(toCollection(candidateGeo)));
  const queryJson = JSON.stringify(SPATIAL_QUERY);

  // rbush bbox index over the rules — the JS answer to "just index it".
  const tree = new RBush(16);
  tree.load(
    ruleFeatures.map((featureObj, index) => {
      const [minX, minY, maxX, maxY] = bbox(featureObj);
      return { minX, minY, maxX, maxY, index };
    }),
  );

  // Weakest baseline: naive scan, no index.
  const naiveTurf = () => scanMatched(candidateTurf, ruleTurf, { bbox: false });

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

  const nativeBatch = () => matchedCount(ruleset.query(candidatesBuffer, queryJson));

  const expected = nativeBatch();
  const naive = naiveTurf();
  const indexed = indexedTurf();
  if (naive !== expected || indexed !== expected) {
    console.error(`mismatch: naive=${naive} indexed=${indexed} native=${expected}`);
    process.exit(1);
  }

  console.log(`fair competitor — ${RULES} grid rules × ${CANDIDATES} candidates, ${expected} matched`);
  console.log(`naive turf (scan)      : ${minOf(naiveTurf, REPS).toFixed(1)} ms`);
  console.log(`rbush + turf (indexed) : ${minOf(indexedTurf, REPS).toFixed(1)} ms`);
  console.log(`native addon           : ${minOf(nativeBatch, REPS).toFixed(2)} ms`);
}

// === complex — complexity & metadata stress ================================

function runComplex(args) {
  const { section: complex } = sectionConfig('complex', args);
  const RULES = Number(complex.rules);
  const PARTS = Number(complex.parts);
  const VERTICES = Number(complex.vertices);
  const FIELDS = Number(complex.fields);
  const CANDIDATES = Number(complex.candidates);
  const REPS = Number(complex.reps ?? 3);
  const rulesFile = complex.rulesFile ? resolveRepoPath(complex.rulesFile) : null;

  // The "coastline" rings are the same jittered-ring shape as `blobRing` in
  // common.mjs (shared with the grid rules) — just much larger here.

  // `fields` typed properties per rule: a mix of enum strings, ints, floats and
  // bools — enough to stress the compile-time property index.
  function metadata(i, rng, fields) {
    const properties = { classification: `class-${i % 5}` };
    for (let f = 0; f < fields; f += 1) {
      const kind = f % 4;
      if (kind === 0) properties[`str_${f}`] = `value-${Math.floor(rng() * 50)}`;
      else if (kind === 1) properties[`int_${f}`] = Math.floor(rng() * 1000);
      else if (kind === 2) properties[`flt_${f}`] = Math.round(rng() * 1000) / 10;
      else properties[`bool_${f}`] = rng() < 0.5;
    }
    return properties;
  }

  // The complex "coastline" rule set: `PARTS` disjoint blobs per rule, each
  // with `VERTICES`-vertex rings and `FIELDS` typed properties.
  function makeComplexRules() {
    const rng = makeRng(0x51a7_0000);
    const features = [];
    for (let i = 0; i < RULES; i += 1) {
      const cx = i * 90; // spread rules apart so the bbox index stays selective
      const polygons = [];
      for (let p = 0; p < PARTS; p += 1) {
        const pcx = cx + p * 30; // > 2× the max radius, so parts never overlap
        const pcy = 0;
        const ring = blobRing(rng, pcx, pcy, 10, VERTICES);
        const holes = p % 2 === 0 ? [blobRing(rng, pcx, pcy, 3, 400)] : [];
        polygons.push([ring, ...holes]);
      }
      features.push({
        type: 'Feature',
        id: `rule-${i}`,
        properties: metadata(i, rng, FIELDS),
        geometry: { type: 'MultiPolygon', coordinates: polygons },
      });
    }
    return features;
  }

  function loadRules() {
    if (rulesFile) {
      const { features, dropped, bytes } = loadRulesFromFile(rulesFile);
      return { features, dropped, bytes };
    }
    const features = makeComplexRules();
    return { features, dropped: [], bytes: Buffer.byteLength(JSON.stringify(toCollection(features))) };
  }

  function* walkRings(geometry) {
    const type = geometry.type;
    if (type === 'Polygon') yield* geometry.coordinates;
    else if (type === 'MultiPolygon') for (const poly of geometry.coordinates) yield* poly;
    else if (type === 'GeometryCollection') for (const g of geometry.geometries) yield* walkRings(g);
  }

  function countVertices(features) {
    let count = 0;
    for (const featureObj of features) {
      for (const ring of walkRings(featureObj.geometry)) count += ring.length;
    }
    return count;
  }

  function countFields(features) {
    const keys = new Set();
    for (const featureObj of features) for (const key of Object.keys(featureObj.properties ?? {})) keys.add(key);
    return keys.size;
  }

  // First exterior ring of a feature (or null) — reuse common's traversal.
  const firstExteriorRing = (featureObj) => exteriorRings(featureObj.geometry).next().value ?? null;

  // Synthetic mode places small squares on a radius-5 ring around the first
  // rule's first part centre. Real-data mode derives candidates from the loaded
  // file: tiny squares centred on sampled boundary vertices of the first
  // feature, sized from its bbox — every square is guaranteed to intersect the
  // rule, and the rest of the ruleset is far away.
  function makeComplexCandidates(ruleGeo, fromData) {
    if (fromData) {
      const ring = firstExteriorRing(ruleGeo[0]);
      const [minX, minY, maxX, maxY] = bbox(toCollection(ruleGeo));
      const w = Math.max(maxX - minX, maxY - minY) * 0.0005;
      const features = [];
      for (let i = 0; i < CANDIDATES; i += 1) {
        const idx = Math.floor((i / CANDIDATES) * (ring.length - 1));
        features.push(square(ring[idx][0], ring[idx][1], w, `cand-${i}`));
      }
      return features;
    }
    const features = [];
    for (let i = 0; i < CANDIDATES; i += 1) {
      const angle = (i / CANDIDATES) * Math.PI * 2;
      const x = 5 * Math.cos(angle);
      const y = 5 * Math.sin(angle);
      features.push(square(x, y, 0.1, `cand-${i}`));
    }
    return features;
  }

  // Synthetic rules carry `classification: class-N`; the where clause filters
  // to class-0. Real-data files usually lack that field, so derive a where
  // clause from the first feature's own properties: the most common string
  // value shared by >1 but not all rules (e.g. CONTINENT=Asia).
  function deriveWhere(ruleGeo) {
    if (!rulesFile) return { field: 'classification', value: 'class-0' };
    const first = ruleGeo[0]?.properties ?? {};
    let best = null;
    for (const [field, value] of Object.entries(first)) {
      if (typeof value !== 'string') continue;
      let n = 0;
      for (const f of ruleGeo) if (f.properties?.[field] === value) n += 1;
      if (n > 1 && n < ruleGeo.length && (!best || n > best.n)) best = { field, value, n };
    }
    return best;
  }

  const { features: ruleGeo, bytes, dropped = [] } = loadRules();
  const candidateGeo = makeComplexCandidates(ruleGeo, Boolean(rulesFile));
  const candidateTurf = toTurf(candidateGeo);
  const ruleTurf = toTurf(ruleGeo);
  const candidatesBuffer = Buffer.from(JSON.stringify(toCollection(candidateGeo)));
  const rulesBuffer = Buffer.from(JSON.stringify(toCollection(ruleGeo)));

  const where = deriveWhere(ruleGeo);
  const whereLabel = where ? `with where{${where.field}=${where.value}}` : 'no where (spatial only)';
  const querySpatial = JSON.stringify(SPATIAL_QUERY);
  const queryWhere = where
    ? JSON.stringify({ spatial: { predicate: 'intersects' }, where: { [where.field]: where.value } })
    : null;

  let ruleset = null;

  const nativeMask = (queryJson) => matchedCount(ruleset.query(candidatesBuffer, queryJson));

  // Turf baselines over the pre-parsed candidates/rules (bbox fast-reject; the
  // `where` variant also filters rules by a property value).
  const turfNaive = () => scanMatched(candidateTurf, ruleTurf);
  const turfWhere = (where) =>
    scanMatched(candidateTurf, ruleTurf, {
      filter: (r) => ruleGeo[r].properties[where.field] === where.value,
    });

  console.log('complexity & metadata stress');
  console.log(`mode=${rulesFile ? 'real-data' : 'synthetic'} rules=${ruleGeo.length}${dropped.length ? ` (${dropped.length} invalid dropped)` : ''} fields=${countFields(ruleGeo)} candidates=${candidateGeo.length}`);
  console.log(`rules GeoJSON size: ${(bytes / 1024 / 1024).toFixed(2)} MB, total vertices: ${countVertices(ruleGeo).toLocaleString('en-US')}`);

  // Correctness: naive turf and the addon must agree (with where too).
  const build = timed(() => {
    ruleset = new SpatialRuleset(rulesBuffer);
  });
  const turfSpatialExpected = turfNaive();
  const turfWhereExpected = where ? turfWhere(where) : null;

  // The first addon query warms the per-thread prepared-geometry cache
  // (ADR-0010) — time it separately: it is the one-time preparation cost.
  const coldQuery = timed(() => nativeMask(querySpatial));
  const nativeWhereExpected = where ? nativeMask(queryWhere) : null;

  if (coldQuery.result !== turfSpatialExpected || (where && nativeWhereExpected !== turfWhereExpected)) {
    console.error(
      `mismatch: native=${coldQuery.result}/${nativeWhereExpected} turf=${turfSpatialExpected}/${turfWhereExpected}`,
    );
    process.exit(1);
  }

  const nativeSpatial = minOf(() => nativeMask(querySpatial), REPS);
  const nativeWhere = where ? minOf(() => nativeMask(queryWhere), REPS) : null;
  const turfSpatial = minOf(turfNaive, REPS);
  const turfWhereRun = where ? minOf(() => turfWhere(where), REPS) : null;
  const spatialMatched = nativeMask(querySpatial);
  const whereMatched = where ? nativeMask(queryWhere) : null;

  console.log(`\nbuild (Rust parse+validate+index): ${build.ms.toFixed(1)} ms`);
  console.log(`first query (builds prepared geometries): ${coldQuery.ms.toFixed(1)} ms`);
  console.log(`query spatial   — addon ${nativeSpatial.toFixed(2)} ms | turf (scan+bbox) ${turfSpatial.toFixed(1)} ms`);
  if (where) console.log(`query + where   — addon ${nativeWhere.toFixed(2)} ms | turf (scan+bbox) ${turfWhereRun.toFixed(1)} ms`);
  console.log(`matched: ${spatialMatched} (spatial)${where ? `, ${whereMatched} (${whereLabel})` : ''}`);
}

// === crossover — break-even sweep ==========================================

function runCrossover(args) {
  const { section: crossover } = sectionConfig('crossover', args);
  const MODE = crossover.mode ?? 'candidates';
  const RULES = Number(crossover.rules ?? 500);
  const SIZES = String(crossover.sizes ?? '20,200,1000,5000').split(',').map(Number);
  const RULES_RANGE = String(crossover.rulesRange ?? '500,1000,2000,5000').split(',').map(Number);
  const FIXED_CANDIDATES = Number(crossover.candidates ?? 1000);
  const REPS = Number(crossover.reps ?? 3);
  const rulesFile = crossover.rulesFile ? resolveRepoPath(crossover.rulesFile) : null;

  function loadRules() {
    if (!rulesFile) return { features: makeRules(RULES), dropped: [] };
    const { features, dropped } = loadRulesFromFile(rulesFile);
    return { features, dropped };
  }

  // Real-data mode: evenly sample exterior-ring vertices across all rules,
  // each a tiny square sized from the rules' bbox. Synthetic mode: grid cells
  // centred on blob rules (from common.mjs), one overlap per candidate.
  function makeCrossoverCandidates(count, ruleGeo) {
    if (rulesFile) {
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

  // Uses `toTurf`/`scanMatched` from turf.mjs (bbox fast-reject baseline).
  const querySpatial = JSON.stringify(SPATIAL_QUERY);

  function timeRow(ruleset, candBuffer, candTurf, ruleTurf) {
    const nativeMatched = matchedCount(ruleset.query(candBuffer, querySpatial));
    const turfMatched = scanMatched(candTurf, ruleTurf);
    if (nativeMatched !== turfMatched) {
      console.error(`mismatch: native=${nativeMatched} turf=${turfMatched}`);
      process.exit(1);
    }
    const nativeMs = minOf(() => ruleset.query(candBuffer, querySpatial), REPS);
    const turfMs = minOf(() => scanMatched(candTurf, ruleTurf), REPS);
    return { nativeMs, turfMs, matched: nativeMatched };
  }

  function warmup(ruleset, candBuffer) {
    // Warm the per-thread prepared-geometry cache (ADR-0010) — one-time, not timed.
    ruleset.query(candBuffer, querySpatial);
  }

  // MODE=candidates (default): fixed ruleset, sweep candidate count — real
  // data via `--rules-file`, synthetic grid fallback.
  function runCandidateSweep() {
    const { features: ruleGeo, dropped = [] } = loadRules();
    const ruleTurf = toTurf(ruleGeo);
    const ruleset = new SpatialRuleset(Buffer.from(JSON.stringify(toCollection(ruleGeo))));
    warmup(ruleset, Buffer.from(JSON.stringify(toCollection(makeCrossoverCandidates(8, ruleGeo)))));

    console.log('crossover (candidates) — native full query vs turf scan + bbox reject');
    console.log(
      `mode=${rulesFile ? 'real-data' : 'synthetic'} rules=${ruleGeo.length}` +
        `${dropped.length ? ` (${dropped.length} invalid dropped)` : ''} sizes=${SIZES.join(',')} reps=${REPS}`,
    );
    console.log('');
    console.log(' candidates  addon (ms)   turf (ms)   speedup   matched');

    let firstWin = null;
    for (const n of SIZES) {
      const cand = makeCrossoverCandidates(n, ruleGeo);
      const candTurf = toTurf(cand);
      const candBuffer = Buffer.from(JSON.stringify(toCollection(cand)));

      const { nativeMs, turfMs, matched } = timeRow(ruleset, candBuffer, candTurf, ruleTurf);
      const speedup = turfMs / nativeMs;
      if (firstWin === null && speedup > 1) firstWin = n;
      console.log(
        String(n).padStart(10) +
          nativeMs.toFixed(2).padStart(12) +
          turfMs.toFixed(2).padStart(11) +
          speedLabel(speedup).padStart(9) +
          String(matched).padStart(9),
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
  }

  // MODE=rules: fixed candidate count, sweep rule count on a synthetic grid —
  // isolates the R*-tree index (turf's scan + bbox reject is O(candidates ×
  // rules); the addon's index lookup is ~log(rules)).
  function runRulesSweep() {
    console.log('crossover (rules) — native full query vs turf scan + bbox reject');
    console.log(`candidates=${FIXED_CANDIDATES} rules=${RULES_RANGE.join(',')} reps=${REPS}`);
    console.log('');
    console.log('    rules  addon (ms)   turf (ms)   speedup   matched');

    for (const n of RULES_RANGE) {
      const features = makeRules(n);
      const ruleTurf = toTurf(features);
      const ruleset = new SpatialRuleset(Buffer.from(JSON.stringify(toCollection(features))));
      warmup(ruleset, Buffer.from(JSON.stringify(toCollection(makeGridCandidates(8, n, makeRng(1))))));

      const cand = makeGridCandidates(FIXED_CANDIDATES, n, makeRng(0x51a7_0001));
      const candTurf = toTurf(cand);
      const candBuffer = Buffer.from(JSON.stringify(toCollection(cand)));

      const { nativeMs, turfMs, matched } = timeRow(ruleset, candBuffer, candTurf, ruleTurf);
      const speedup = turfMs / nativeMs;
      console.log(
        String(n).padStart(9) +
          nativeMs.toFixed(2).padStart(12) +
          turfMs.toFixed(2).padStart(11) +
          speedLabel(speedup).padStart(9) +
          String(matched).padStart(9),
      );
    }
    console.log('');
    console.log('the addon stays ~flat as rules grow (index lookup); turf grows linearly (scan)');
  }

  if (MODE === 'rules') runRulesSweep();
  else runCandidateSweep();
}
