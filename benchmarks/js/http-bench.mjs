// End-to-end production query over HTTP (Bun + Express addon) vs the
// equivalent hand-rolled turf.js query in-process.
//
// The full query shape — spatial predicate + property `where` + excludeRuleIds
// — is one turf has no API for: the JS side must hand-roll the filter. This
// measures the real production path (JSON parse → query → mask over HTTP), not
// just a booleanIntersects loop. The turf number is a *lower bound* (a turf
// endpoint would add its own HTTP + serialization overhead).
//
//   node http-bench.mjs              # spawns `bun integration/server.mjs`
//   BASE_URL=http://localhost:3000 node http-bench.mjs   # against a running server

import { spawn } from 'node:child_process';
import { readFileSync } from 'node:fs';
import { dirname, join } from 'node:path';
import { fileURLToPath } from 'node:url';
import { performance } from 'node:perf_hooks';
import { feature, booleanIntersects } from '@turf/turf';

const here = dirname(fileURLToPath(import.meta.url));
const repoRoot = join(here, '..', '..');
const base = process.env.BASE_URL ?? 'http://localhost:3000';
const port = Number(process.env.PORT ?? 3000);

const rulesCollection = JSON.parse(readFileSync(join(repoRoot, 'benchmarks', 'data', 'rules.geojson'), 'utf8'));
const candidatesCollection = JSON.parse(readFileSync(join(repoRoot, 'benchmarks', 'data', 'candidates.geojson'), 'utf8'));
const rules = rulesCollection.features;
const candidates = candidatesCollection.features;
const ruleFeatures = rules.map((r) => feature(r.geometry));
const candidateFeatures = candidates.map((c) => feature(c.geometry));

const query = {
  spatial: { predicate: 'intersects' },
  where: { classification: 'restricted' },
  excludeRuleIds: ['rule-00', 'rule-05'],
};

// The JS side of the full query. turf has no `where`/exclusion model, so this
// is exactly the code an application has to write around turf to match the
// engine's semantics.
function turfFullQuery() {
  const excluded = new Set(query.excludeRuleIds);
  const mask = new Array(candidateFeatures.length);
  for (let c = 0; c < candidateFeatures.length; c += 1) {
    let matched = false;
    for (let r = 0; r < ruleFeatures.length; r += 1) {
      if (excluded.has(rules[r].id)) continue;
      if (rules[r].properties.classification !== query.where.classification) continue;
      if (booleanIntersects(candidateFeatures[c], ruleFeatures[r])) {
        matched = true;
        break;
      }
    }
    mask[c] = matched ? 1 : 0;
  }
  return mask;
}

let server = null;
if (!process.env.BASE_URL) {
  server = spawn(process.env.BUN ?? 'bun', [join(repoRoot, 'integration', 'server.mjs')], {
    stdio: ['ignore', 'ignore', 'ignore'],
    env: { ...process.env, PORT: String(port) },
  });
}

async function waitForHealth(timeoutMs = 15_000) {
  const deadline = Date.now() + timeoutMs;
  while (Date.now() < deadline) {
    try {
      const response = await fetch(`${base}/health`);
      if (response.ok) return;
    } catch {
      // not up yet
    }
    await new Promise((resolve) => setTimeout(resolve, 200));
  }
  throw new Error('server did not become healthy');
}

async function addonRequest() {
  const response = await fetch(`${base}/query`, {
    method: 'POST',
    headers: { 'content-type': 'application/json' },
    body: JSON.stringify({ candidates: candidatesCollection, query }),
  });
  if (!response.ok) throw new Error(`/query failed: ${response.status}`);
  return (await response.json()).mask;
}

async function main() {
  try {
    await waitForHealth();

    // Correctness: the HTTP addon path and the hand-rolled turf path must agree.
    const addonMask = await addonRequest();
    const turfMask = turfFullQuery();
    if (addonMask.length !== turfMask.length || addonMask.some((v, i) => v !== turfMask[i])) {
      throw new Error('mask mismatch between addon and turf');
    }
    const matched = addonMask.filter((v) => v === 1).length;

    const iterations = Number(process.env.ITERS ?? 10);

    // Addon over HTTP — the production path.
    const samples = [];
    for (let i = 0; i < iterations; i += 1) {
      const start = performance.now();
      await addonRequest();
      samples.push(performance.now() - start);
    }
    const addonMean = samples.reduce((a, b) => a + b, 0) / samples.length;

    // turf in-process — a lower bound (no HTTP, no per-request JSON re-parse).
    turfFullQuery(); // warmup
    const start = performance.now();
    for (let i = 0; i < 3; i += 1) turfFullQuery();
    const turfMean = (performance.now() - start) / 3;

    console.log(`production query: intersects + where{classification=restricted} + 2 exclusions, ${matched} matched`);
    console.log(`addon over HTTP (${iterations} reqs): ${addonMean.toFixed(2)} ms/request`);
    console.log(`turf in-process (3 runs)          : ${turfMean.toFixed(1)} ms/batch`);
    console.log(`speedup: ${(turfMean / addonMean).toFixed(0)}×`);
  } finally {
    if (server) server.kill();
  }
}

main().catch((error) => {
  console.error(error);
  if (server) server.kill();
  process.exit(1);
});
