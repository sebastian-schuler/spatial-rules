// Shared helpers for the limitations benchmark suite (scale / fair / http).

import { createRequire } from 'node:module';
import { dirname, join } from 'node:path';
import { fileURLToPath } from 'node:url';

const here = dirname(fileURLToPath(import.meta.url));
const require = createRequire(import.meta.url);

// The raw napi binding (no JS wrapper) — benchmarks the native hot path.
export function loadNative() {
  return require(join(here, '..', '..', 'node', 'spatial_rules.node'));
}

export function matchedCount(mask) {
  let count = 0;
  for (const value of mask) if (value === 1) count += 1;
  return count;
}

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
// country-scale rules (60–400 vertices, some with holes).
function blobRing(rng, cx, cy, radius, vertices) {
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
export function makeCandidates(m, ruleCount, rng) {
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
