// Smoke test for the integration server: posts the production workload
// (30 rules held by the server × 1,000 candidate footprints) and asserts the
// mask shape. Run after starting the server:
//
//   bun run bench server   (then)   bun run bench smoke
//
// The server address comes from benchmarks.json; `--port` / `--base-url`
// override it.

import { readFileSync } from 'node:fs';
import { readConfig, parseFlags, resolveRepoPath, SPATIAL_QUERY } from '../shared/config.mjs';

const config = readConfig();

// CLI overrides only — the harness never uses environment variables.
const { values } = parseFlags(process.argv.slice(2), {
  port: { type: 'string' },
  'base-url': { type: 'string' },
});
const port = Number(values.port ?? config.global.server.port ?? 3000);
const base = values['base-url'] ?? `http://localhost:${port}`;

const candidates = JSON.parse(
  readFileSync(resolveRepoPath(config.global.paths.candidatesFile), 'utf8'),
);

const health = await fetch(`${base}/health`).then((response) => response.json());
if (!health.ok) throw new Error(`health check failed: ${JSON.stringify(health)}`);

const response = await fetch(`${base}/query`, {
  method: 'POST',
  headers: { 'content-type': 'application/json' },
  body: JSON.stringify({ candidates, query: SPATIAL_QUERY }),
});
if (!response.ok) throw new Error(`/query failed: ${response.status}`);
const { mask } = await response.json();

if (!Array.isArray(mask) || mask.length !== candidates.features.length) {
  throw new Error(`expected mask length ${candidates.features.length}, got ${mask?.length}`);
}

const matched = mask.filter((value) => value === 1).length;
console.log(`smoke: ${mask.length} candidates, ${matched} matched`);
console.log('integration smoke passed');
