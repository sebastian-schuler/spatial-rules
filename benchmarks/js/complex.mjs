// Complexity & metadata stress — how the engine and turf.js behave when rules
// are extremely complex (many vertices, multiple parts, holes) and
// metadata-rich (dozens of typed properties).
//
// Synthetic mode generates a multi-MB rule set of jittered "coastline"
// polygons with tens of thousands of vertices and many properties. Real-data
// mode loads any GeoJSON boundary (e.g. a full-detail Germany file) and runs
// the same measurements.
//
//   node complex.mjs                                   # synthetic defaults
//   RULES_FILE=deu.geojson node complex.mjs            # a real boundary file
//   RULES=5 VERTICES=20000 FIELDS=80 node complex.mjs  # scale the synthetic set

import { readFileSync } from 'node:fs';
import { performance } from 'node:perf_hooks';
import { feature, booleanIntersects, bbox } from '@turf/turf';
import RBush from 'rbush';
import { loadNative, makeRng, toCollection } from './common.mjs';

const { SpatialRuleset } = loadNative();

const RULES = Number(process.env.RULES ?? 3);
const PARTS = Number(process.env.PARTS ?? 3);
const VERTICES = Number(process.env.VERTICES ?? 5000);
const FIELDS = Number(process.env.FIELDS ?? 40);
const CANDIDATES = Number(process.env.CANDIDATES ?? 20);

// A closed, star-shaped "coastline" ring: a jittered circle with `vertices`
// points (radius jitter 0.7–1.2 × base, always positive → valid and
// non-self-intersecting). This is the same shape family as the committed
// benchmark dataset, just much larger.
function coastlineRing(rng, cx, cy, radius, vertices) {
  const coords = [];
  for (let i = 0; i < vertices; i += 1) {
    const angle = (i / vertices) * Math.PI * 2;
    const r = radius * (0.7 + 0.5 * rng());
    coords.push([cx + r * Math.cos(angle), cy + r * Math.sin(angle)]);
  }
  coords.push(coords[0]);
  return coords;
}

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

function makeRules() {
  const rng = makeRng(0x51a7_0000);
  const features = [];
  for (let i = 0; i < RULES; i += 1) {
    const cx = i * 90; // spread rules apart so the bbox index stays selective
    const polygons = [];
    for (let p = 0; p < PARTS; p += 1) {
      const pcx = cx + p * 30; // > 2× the max radius, so parts never overlap
      const pcy = 0;
      const ring = coastlineRing(rng, pcx, pcy, 10, VERTICES);
      const holes = p % 2 === 0 ? [coastlineRing(rng, pcx, pcy, 3, 400)] : [];
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
  const file = process.env.RULES_FILE;
  if (file) {
    const raw = readFileSync(file, 'utf8');
    const geo = JSON.parse(raw);
    const features = geo.type === 'FeatureCollection' ? geo.features : [geo];
    return { features, bytes: Buffer.byteLength(raw) };
  }
  const features = makeRules();
  return { features, bytes: Buffer.byteLength(JSON.stringify(toCollection(features))) };
}

function countVertices(features) {
  let count = 0;
  for (const featureObj of features) {
    for (const ring of walkRings(featureObj.geometry)) count += ring.length;
  }
  return count;
}

function* walkRings(geometry) {
  const type = geometry.type;
  if (type === 'Polygon') yield* geometry.coordinates;
  else if (type === 'MultiPolygon') for (const poly of geometry.coordinates) yield* poly;
  else if (type === 'GeometryCollection') for (const g of geometry.geometries) yield* walkRings(g);
}

function countFields(features) {
  const keys = new Set();
  for (const featureObj of features) for (const key of Object.keys(featureObj.properties ?? {})) keys.add(key);
  return keys.size;
}

function makeCandidates() {
  // Small squares on a ring of radius 5 around the first rule's first part
  // centre: inside the exterior (r≈7–12) but outside the hole (r≈2.1–3.6).
  const features = [];
  for (let i = 0; i < CANDIDATES; i += 1) {
    const angle = (i / CANDIDATES) * Math.PI * 2;
    const x = 5 * Math.cos(angle);
    const y = 5 * Math.sin(angle);
    const w = 0.1;
    features.push({
      type: 'Feature',
      id: `cand-${i}`,
      properties: {},
      geometry: {
        type: 'Polygon',
        coordinates: [
          [[x - w, y - w], [x - w, y + w], [x + w, y + w], [x + w, y - w], [x - w, y - w]],
        ],
      },
    });
  }
  return features;
}

const { features: ruleGeo, bytes } = loadRules();
const ruleFeatures = ruleGeo.map((f) => feature(f.geometry));
const candidateGeo = makeCandidates();
const candidateFeatures = candidateGeo.map((f) => feature(f.geometry));
const candidatesBuffer = Buffer.from(JSON.stringify(toCollection(candidateGeo)));
const rulesBuffer = Buffer.from(JSON.stringify(toCollection(ruleGeo)));

const querySpatial = JSON.stringify({ spatial: { predicate: 'intersects' } });
const queryWhere = JSON.stringify({
  spatial: { predicate: 'intersects' },
  where: { classification: 'class-0' },
});

let ruleset = null;

function nativeMask(queryJson) {
  let matched = 0;
  for (const value of ruleset.query(candidatesBuffer, queryJson)) {
    if (value === 1) matched += 1;
  }
  return matched;
}

function turfNaive() {
  let matched = 0;
  for (const c of candidateFeatures) {
    for (const r of ruleFeatures) if (booleanIntersects(c, r)) { matched += 1; break; }
  }
  return matched;
}

function turfWhere() {
  let matched = 0;
  for (const c of candidateFeatures) {
    for (let i = 0; i < ruleFeatures.length; i += 1) {
      if (ruleGeo[i].properties.classification !== 'class-0') continue;
      if (booleanIntersects(c, ruleFeatures[i])) { matched += 1; break; }
    }
  }
  return matched;
}

function once(fn) {
  const start = performance.now();
  const result = fn();
  return { ms: performance.now() - start, result };
}

console.log('complexity & metadata stress');
console.log(`rules=${ruleGeo.length} parts=${PARTS} vertices/ring=${VERTICES} fields=${countFields(ruleGeo)} candidates=${candidateGeo.length}`);
console.log(`rules GeoJSON size: ${(bytes / 1024 / 1024).toFixed(2)} MB, total vertices: ${countVertices(ruleGeo).toLocaleString('en-US')}`);

// Correctness: naive turf and the addon must agree (with where too).
const build = once(() => {
  ruleset = new SpatialRuleset(rulesBuffer);
});
const turfSpatialExpected = turfNaive();
const turfWhereExpected = turfWhere();

// The first addon query warms the per-thread prepared-geometry cache
// (ADR-0010) — time it separately: it is the one-time preparation cost, not
// the steady-state query.
const coldQuery = once(() => nativeMask(querySpatial));
const nativeWhereExpected = nativeMask(queryWhere);

if (coldQuery.result !== turfSpatialExpected || nativeWhereExpected !== turfWhereExpected) {
  console.error(
    `mismatch: native=${coldQuery.result}/${nativeWhereExpected} turf=${turfSpatialExpected}/${turfWhereExpected}`,
  );
  process.exit(1);
}

const nativeSpatial = once(() => nativeMask(querySpatial));
const nativeWhere = once(() => nativeMask(queryWhere));
const turfSpatial = once(turfNaive);
const turfWhereRun = once(turfWhere);

console.log(`\nbuild (Rust parse+validate+index): ${build.ms.toFixed(1)} ms`);
console.log(`first query (builds prepared geometries): ${coldQuery.ms.toFixed(1)} ms`);
console.log(`query spatial   — addon ${nativeSpatial.ms.toFixed(2)} ms | turf ${turfSpatial.ms.toFixed(1)} ms`);
console.log(`query + where   — addon ${nativeWhere.ms.toFixed(2)} ms | turf ${turfWhereRun.ms.toFixed(1)} ms`);
console.log(`matched: ${nativeSpatial.result} (spatial), ${nativeWhere.result} (with where{classification=class-0})`);
