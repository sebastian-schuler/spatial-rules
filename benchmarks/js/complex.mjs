// Complexity & metadata stress — how the engine and turf.js behave when rules
// are extremely complex (many vertices, multiple parts, holes) and
// metadata-rich (dozens of typed properties).
//
// Synthetic mode generates a multi-MB rule set of jittered "coastline"
// polygons with tens of thousands of vertices and many properties. Real-data
// mode loads any GeoJSON boundary (e.g. a full-detail Germany file) and runs
// the same measurements.
//
//   bun complex.mjs                                     # synthetic defaults
//   RULES_FILE=deu.geojson bun complex.mjs              # a real boundary file
//   RULES=5 VERTICES=20000 FIELDS=80 bun complex.mjs    # scale the synthetic set
//
// bun auto-loads .env from the working directory, so `RULES_FILE="countries.geojson"`
// in benchmarks/js/.env switches the default to real-data mode with no flag.

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
    // The engine requires each rule to carry an id; Natural Earth features
    // don't, so assign one from a stable property (or the index).
    for (let i = 0; i < features.length; i += 1) {
      const f = features[i];
      if (f.id == null && f.properties?.id == null) {
        f.id = f.properties?.ne_id != null ? `ne-${f.properties.ne_id}` : `rule-${i}`;
      }
    }
    // The engine validates strictly (ADR-0005) and rejects the whole ruleset if
    // any rule is invalid; Natural Earth has a few self-intersecting
    // boundaries. Drop the ones the engine rejects so both sides see the same
    // valid rules (and the build succeeds).
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
    return { features: valid, dropped, bytes: Buffer.byteLength(raw) };
  }
  const features = makeRules();
  return { features, dropped: [], bytes: Buffer.byteLength(JSON.stringify(toCollection(features))) };
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

function firstExteriorRing(featureObj) {
  const g = featureObj.geometry;
  if (!g) return null;
  if (g.type === 'Polygon') return g.coordinates[0];
  if (g.type === 'MultiPolygon') return g.coordinates[0][0];
  if (g.type === 'GeometryCollection') {
    for (const sub of g.geometries) {
      const ring = firstExteriorRing({ geometry: sub });
      if (ring) return ring;
    }
  }
  return null;
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

// Synthetic mode places small squares on a radius-5 ring around the first
// rule's first part centre: inside the exterior (r≈7–12), outside the hole
// (r≈2.1–3.6). Real-data mode derives candidates from the loaded file: tiny
// squares centred on sampled boundary vertices of the first feature, sized
// from its bbox — every square is guaranteed to intersect the rule, and the
// rest of the ruleset is far away, so the bbox index must filter down to the
// one overlapping feature.
function makeCandidates(ruleGeo, fromData) {
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

// Synthetic rules carry `classification: class-N`; the where clause filters to
// class-0. Real-data files usually lack that field, so derive a where clause
// from the first feature's own properties: the most common string value that
// is shared by more than one rule but not all of them (e.g. CONTINENT=Asia) —
// non-trivial work for the property index, and it always keeps the first
// feature (the one the candidates overlap) in the filtered set. Returns null
// when no such value exists, in which case only the spatial query is timed.
function deriveWhere(ruleGeo) {
  if (!process.env.RULES_FILE) return { field: 'classification', value: 'class-0' };
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
const ruleFeatures = ruleGeo.map((f) => feature(f.geometry));
const candidateGeo = makeCandidates(ruleGeo, Boolean(process.env.RULES_FILE));
const candidateFeatures = candidateGeo.map((f) => feature(f.geometry));
const ruleBboxes = ruleGeo.map((f) => bbox(f));
const candidatesBuffer = Buffer.from(JSON.stringify(toCollection(candidateGeo)));
const rulesBuffer = Buffer.from(JSON.stringify(toCollection(ruleGeo)));

const where = deriveWhere(ruleGeo);
const whereLabel = where ? `with where{${where.field}=${where.value}}` : 'no where (spatial only)';
const querySpatial = JSON.stringify({ spatial: { predicate: 'intersects' } });
const queryWhere = where
  ? JSON.stringify({ spatial: { predicate: 'intersects' }, where: { [where.field]: where.value } })
  : null;

let ruleset = null;

function nativeMask(queryJson) {
  let matched = 0;
  for (const value of ruleset.query(candidatesBuffer, queryJson)) {
    if (value === 1) matched += 1;
  }
  return matched;
}

function bboxOverlap(a, b) {
  return a[0] <= b[2] && a[2] >= b[0] && a[1] <= b[3] && a[3] >= b[1];
}

// Naive linear scan with a per-rule bbox fast-reject (the hand-rolled baseline
// an rbush index would replace). A true scan — relating every candidate against
// every rule — is O(candidates × rules) relate calls and would take minutes on
// the 258-country file.
function turfNaive() {
  let matched = 0;
  for (const c of candidateFeatures) {
    const cb = bbox(c);
    for (let i = 0; i < ruleFeatures.length; i += 1) {
      if (!bboxOverlap(cb, ruleBboxes[i])) continue;
      if (booleanIntersects(c, ruleFeatures[i])) { matched += 1; break; }
    }
  }
  return matched;
}

function turfWhere(where) {
  let matched = 0;
  for (const c of candidateFeatures) {
    const cb = bbox(c);
    for (let i = 0; i < ruleFeatures.length; i += 1) {
      if (ruleGeo[i].properties[where.field] !== where.value) continue;
      if (!bboxOverlap(cb, ruleBboxes[i])) continue;
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
console.log(`mode=${process.env.RULES_FILE ? 'real-data' : 'synthetic'} rules=${ruleGeo.length}${dropped.length ? ` (${dropped.length} invalid dropped)` : ''} fields=${countFields(ruleGeo)} candidates=${candidateGeo.length}`);
console.log(`rules GeoJSON size: ${(bytes / 1024 / 1024).toFixed(2)} MB, total vertices: ${countVertices(ruleGeo).toLocaleString('en-US')}`);

// Correctness: naive turf and the addon must agree (with where too).
const build = once(() => {
  ruleset = new SpatialRuleset(rulesBuffer);
});
const turfSpatialExpected = turfNaive();
const turfWhereExpected = where ? turfWhere(where) : null;

// The first addon query warms the per-thread prepared-geometry cache
// (ADR-0010) — time it separately: it is the one-time preparation cost, not
// the steady-state query.
const coldQuery = once(() => nativeMask(querySpatial));
const nativeWhereExpected = where ? nativeMask(queryWhere) : null;

if (coldQuery.result !== turfSpatialExpected || (where && nativeWhereExpected !== turfWhereExpected)) {
  console.error(
    `mismatch: native=${coldQuery.result}/${nativeWhereExpected} turf=${turfSpatialExpected}/${turfWhereExpected}`,
  );
  process.exit(1);
}

const nativeSpatial = once(() => nativeMask(querySpatial));
const nativeWhere = where ? once(() => nativeMask(queryWhere)) : null;
const turfSpatial = once(turfNaive);
const turfWhereRun = where ? once(() => turfWhere(where)) : null;

console.log(`\nbuild (Rust parse+validate+index): ${build.ms.toFixed(1)} ms`);
console.log(`first query (builds prepared geometries): ${coldQuery.ms.toFixed(1)} ms`);
console.log(`query spatial   — addon ${nativeSpatial.ms.toFixed(2)} ms | turf (scan+bbox) ${turfSpatial.ms.toFixed(1)} ms`);
if (where) console.log(`query + where   — addon ${nativeWhere.ms.toFixed(2)} ms | turf (scan+bbox) ${turfWhereRun.ms.toFixed(1)} ms`);
console.log(`matched: ${nativeSpatial.result} (spatial)${where ? `, ${nativeWhere.result} (${whereLabel})` : ''}`);
