// Smoke test for the integration server: posts the production workload
// (30 rules held by the server × 1,000 candidate footprints) and asserts the
// mask shape. Run after starting the server:
//
//   bun server.mjs   (then)   node smoke.mjs

import { readFileSync } from 'node:fs';
import { dirname, join } from 'node:path';
import { fileURLToPath } from 'node:url';

const here = dirname(fileURLToPath(import.meta.url));
const root = join(here, '..');
const base = process.env.BASE_URL ?? 'http://localhost:3000';

const candidates = JSON.parse(
  readFileSync(join(root, 'benchmarks', 'data', 'candidates.geojson'), 'utf8'),
);

const health = await fetch(`${base}/health`).then((response) => response.json());
if (!health.ok) throw new Error(`health check failed: ${JSON.stringify(health)}`);

const response = await fetch(`${base}/query`, {
  method: 'POST',
  headers: { 'content-type': 'application/json' },
  body: JSON.stringify({ candidates, query: { spatial: { predicate: 'intersects' } } }),
});
if (!response.ok) throw new Error(`/query failed: ${response.status}`);
const { mask } = await response.json();

if (!Array.isArray(mask) || mask.length !== candidates.features.length) {
  throw new Error(`expected mask length ${candidates.features.length}, got ${mask?.length}`);
}

const matched = mask.filter((value) => value === 1).length;
console.log(`smoke: ${mask.length} candidates, ${matched} matched`);
console.log('integration smoke passed');
