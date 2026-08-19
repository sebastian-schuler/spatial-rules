// Shared helpers for the benchmark harness (sweeps, server-bench, cross-check)
// and the integration scripts (server/memory/smoke). Turf-free on purpose: the
// Docker image (which imports common.mjs but not the dev-only turf deps) and
// the JS harness both use it.
//
// Config lives in one committed file at the repo root — `benchmarks.json` —
// and per-run tweaks come through CLI flags (never environment variables).
// See docs/benchmarks.md for the key -> flag map.

import { createRequire } from 'node:module';
import { readFileSync } from 'node:fs';
import { isAbsolute, join } from 'node:path';
import { dirname } from 'node:path';
import { fileURLToPath } from 'node:url';
import { parseArgs } from 'node:util';

const here = dirname(fileURLToPath(import.meta.url));
const require = createRequire(import.meta.url);

// The harness scripts live in benchmarks/js/, so the repo root is two up.
export const REPO_ROOT = join(here, '..', '..');

// The simple spatial-only query every harness drives — defined once, turf-free.
export const SPATIAL_QUERY = { spatial: { predicate: 'intersects' } };

// ---- config ---------------------------------------------------------------

export function readConfig() {
  return JSON.parse(readFileSync(join(REPO_ROOT, 'benchmarks.json'), 'utf8'));
}

// Config paths are repo-root-relative; absolute paths (and null) pass through.
export function resolveRepoPath(rel) {
  if (!rel || isAbsolute(rel)) return rel;
  return join(REPO_ROOT, rel);
}

// kebab-case flag (`--rules-file`) -> config key (`rulesFile`).
const toCamel = (key) => key.replace(/-([a-z])/g, (_, c) => c.toUpperCase());

// Parse `--flag=value` (and boolean flags listed in `spec`) with no new deps.
// Unknown flags are collected as strings; per-run tweaks never use env vars.
// A bare `--` separator (e.g. `bun run bench scale -- --sizes=...`) is dropped:
// `bun run` uses it to separate script args from its own flags.
export function parseFlags(args, spec = {}) {
  const { values } = parseArgs({
    args: args.filter((arg) => arg !== '--'),
    options: spec,
    strict: false,
  });
  return { values };
}

// Apply `--flag=value` overrides onto a config section (unknown flags ignored).
export function applyOverrides(section, values) {
  const out = { ...section };
  for (const [key, value] of Object.entries(values)) {
    if (value === undefined) continue;
    out[toCamel(key)] = value;
  }
  return out;
}

// Read one tool's config section with `--flag=value` overrides applied, plus
// the full config (for global paths). The one preamble every harness repeats.
export function sectionConfig(name, args, spec = {}) {
  const cfg = readConfig();
  const { values } = parseFlags(args, spec);
  return { cfg, section: applyOverrides(cfg[name] ?? {}, values), values };
}

// "30x100,100x200,300x1000" -> [{ rules: 30, candidates: 100 }, ...]
export function parsePoints(text) {
  return text.split(',').map((point) => {
    const [rules, candidates] = point.split('x').map(Number);
    return { rules, candidates };
  });
}

// ---- native binding -------------------------------------------------------

// The raw napi binding (no JS wrapper) — benchmarks the native hot path. The
// path comes from `global.paths.nodeBinding` so it can never drift from
// `bun run bench build`.
export function loadNative() {
  const { global } = readConfig();
  return require(join(REPO_ROOT, global.paths.nodeBinding));
}

export function matchedCount(mask) {
  let count = 0;
  for (const value of mask) if (value === 1) count += 1;
  return count;
}

// ---- synthetic workloads --------------------------------------------------

// Deterministic 32-bit LCG — reproducible workloads across runs.
export function makeRng(seed) {
  let state = seed >>> 0;
  return () => {
    state = (Math.imul(state, 1664525) + 1013904223) >>> 0;
    return state / 4294967296;
  };
}

// A closed, star-shaped ring (radius jittered but always positive → valid,
// non-self-intersecting), matching the complexity of the committed dataset's
// country-scale rules (60–400 vertices, some with holes). Shared by the grid
// rules here and the complex "coastline" rules in `bench complex`.
export function blobRing(rng, cx, cy, radius, vertices) {
  const coords = [];
  for (let i = 0; i < vertices; i += 1) {
    const angle = (i / vertices) * Math.PI * 2;
    const r = radius * (0.7 + 0.5 * rng());
    coords.push([cx + r * Math.cos(angle), cy + r * Math.sin(angle)]);
  }
  coords.push(coords[0]);
  return coords;
}

// `n` complex blob rules (120–300 vertices, ~35% with a hole) laid out on a
// ~sqrt(n) grid. Each rule stays inside its 1-unit cell, so a candidate placed
// at a cell centre overlaps exactly its own rule: a bbox index filters to ~1
// rule per candidate, while a naive scan still touches all `n`.
export function makeRules(n) {
  const side = Math.ceil(Math.sqrt(n));
  const rng = makeRng(0x5eed_0000 ^ n);
  const features = [];
  for (let i = 0; i < n; i += 1) {
    const col = i % side;
    const row = Math.floor(i / side);
    const cx = col + 0.5;
    const cy = row + 0.5;
    const radius = 0.35;
    const exterior = blobRing(rng, cx, cy, radius, 120 + Math.floor(rng() * 180));
    const holes = rng() < 0.35 ? [blobRing(rng, cx, cy, radius * 0.4, 24)] : [];
    features.push({
      type: 'Feature',
      id: `rule-${i}`,
      properties: { classification: `class-${i % 5}` },
      geometry: { type: 'Polygon', coordinates: [exterior, ...holes] },
    });
  }
  return features;
}

// `m` small square candidates, each centred on a cell centre (inside the rule).
export function makeGridCandidates(m, ruleCount, rng) {
  const side = Math.ceil(Math.sqrt(ruleCount));
  const features = [];
  for (let i = 0; i < m; i += 1) {
    const cx = Math.floor(rng() * side) + 0.5;
    const cy = Math.floor(rng() * side) + 0.5;
    const w = 0.05;
    features.push({
      type: 'Feature',
      id: `cand-${i}`,
      properties: {},
      geometry: {
        type: 'Polygon',
        coordinates: [
          [[cx - w, cy - w], [cx - w, cy + w], [cx + w, cy + w], [cx + w, cy - w], [cx - w, cy - w]],
        ],
      },
    });
  }
  return features;
}

export function toCollection(features) {
  return { type: 'FeatureCollection', features };
}

// ---- real-data (e.g. Natural Earth) mode ----------------------------------

// Load a GeoJSON boundary file as rules: assign ids where missing (Natural
// Earth features have `ne_id`), then drop rules the engine rejects so both
// sides (turf and the addon) see the same valid set (ADR-0005).
export function loadRulesFromFile(file) {
  const { SpatialRuleset } = loadNative();
  const raw = readFileSync(file, 'utf8');
  const geo = JSON.parse(raw);
  const features = geo.type === 'FeatureCollection' ? geo.features : [geo];
  for (let i = 0; i < features.length; i += 1) {
    const f = features[i];
    if (f.id == null && f.properties?.id == null) {
      f.id = f.properties?.ne_id != null ? `ne-${f.properties.ne_id}` : `rule-${i}`;
    }
  }
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

export function* exteriorRings(geometry) {
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

// All exterior-ring vertices across a ruleset (for boundary-derived candidates).
export function ringPoints(features) {
  const pts = [];
  for (const f of features) {
    for (const ring of exteriorRings(f.geometry)) {
      for (let i = 0; i < ring.length - 1; i += 1) pts.push(ring[i]);
    }
  }
  return pts;
}

// A small square feature (boundary/rule-derived candidates in `bench complex`).
export function square(x, y, w, id) {
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

// ---- timing ---------------------------------------------------------------

export function bboxOverlap(a, b) {
  return a[0] <= b[2] && a[2] >= b[0] && a[1] <= b[3] && a[3] >= b[1];
}

// Time one call; returns { ms, result } so callers can assert the result too.
export function timed(fn) {
  const start = performance.now();
  const result = fn();
  return { ms: performance.now() - start, result };
}

// Min-of-N timing (damp scheduler/GC noise).
export function minOf(fn, reps) {
  let best = Infinity;
  for (let r = 0; r < reps; r += 1) {
    const ms = timed(fn).ms;
    if (ms < best) best = ms;
  }
  return best;
}

export function speedLabel(speedup) {
  return speedup >= 100 ? `${speedup.toFixed(0)}x` : `${speedup.toFixed(1)}x`;
}

