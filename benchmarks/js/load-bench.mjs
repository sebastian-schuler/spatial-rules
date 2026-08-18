// Sustained concurrent load test against the integration server.
//
// Measures achievable req/s, query latency percentiles, and event-loop
// responsiveness (via /health probes) under a fixed concurrency, for both the
// JSON endpoint (/query) and the raw bytes-in/bytes-out endpoint (/queryRaw).
//
//   node load-bench.mjs                                  # defaults: /query (json)
//   ENDPOINT=raw node load-bench.mjs                     # raw bytes-in/bytes-out
//   CONCURRENCY=50 DURATION=10000 ENDPOINT=raw node load-bench.mjs
//
// Assumes `bun integration/server.mjs` is running (BASE_URL to point elsewhere).

import { readFileSync } from 'node:fs';
import { dirname, join } from 'node:path';
import { fileURLToPath } from 'node:url';
import { performance } from 'node:perf_hooks';

const here = dirname(fileURLToPath(import.meta.url));
const repoRoot = join(here, '..', '..');
const base = process.env.BASE_URL ?? 'http://localhost:3100';
const endpoint = process.env.ENDPOINT ?? 'json';
const concurrency = Number(process.env.CONCURRENCY ?? 25);
const durationMs = Number(process.env.DURATION ?? 10_000);

const candidatesPath = join(repoRoot, 'benchmarks', 'data', 'candidates.geojson');
// Raw bytes, read once — the third-party fetch pattern (no `.json()`).
const rawBody = readFileSync(candidatesPath);
const query = {
  spatial: { predicate: 'intersects' },
  where: { classification: 'restricted' },
  excludeRuleIds: ['rule-00', 'rule-05'],
};
const jsonBody = JSON.stringify({ candidates: JSON.parse(rawBody.toString('utf8')), query });
const headersJson = { 'content-type': 'application/json' };
const headersRaw = {
  'content-type': 'application/octet-stream',
  'x-query': Buffer.from(JSON.stringify(query)).toString('base64'),
};

function request() {
  if (endpoint === 'raw') {
    return fetch(`${base}/queryRaw`, { method: 'POST', headers: headersRaw, body: rawBody });
  }
  return fetch(`${base}/query`, { method: 'POST', headers: headersJson, body: jsonBody });
}

function pct(sorted, p) {
  const idx = Math.min(sorted.length - 1, Math.floor(sorted.length * p));
  return sorted[idx].toFixed(1);
}

async function main() {
  const warm = await request();
  if (!warm.ok) throw new Error(`warmup failed: ${warm.status}`);
  await warm.arrayBuffer();

  const start = performance.now();
  const deadline = start + durationMs;
  const latencies = [];
  const health = [];
  const errors = [];
  let done = 0;

  async function worker() {
    while (performance.now() < deadline) {
      const t0 = performance.now();
      try {
        const res = await request();
        await res.arrayBuffer();
        if (!res.ok) errors.push(`HTTP ${res.status}`);
      } catch (e) {
        errors.push(String(e).slice(0, 80));
      }
      latencies.push(performance.now() - t0);
      done += 1;
    }
  }

  // Health probe: while the query load runs, measure how quickly a trivial
  // request is served — a proxy for event-loop responsiveness under load.
  async function prober() {
    while (performance.now() < deadline) {
      const t0 = performance.now();
      try {
        const res = await fetch(`${base}/health`);
        await res.arrayBuffer();
      } catch { /* ignore */ }
      health.push(performance.now() - t0);
      await new Promise((r) => setTimeout(r, 100));
    }
  }

  await Promise.all([...Array.from({ length: concurrency }, worker), prober()]);

  const elapsed = (performance.now() - start) / 1000;
  const rps = done / elapsed;
  latencies.sort((a, b) => a - b);
  health.sort((a, b) => a - b);
  console.log(`endpoint=/query${endpoint === 'raw' ? 'Raw' : ''}  concurrency=${concurrency}  duration=${elapsed.toFixed(1)}s`);
  console.log(`  requests=${done}  throughput=${rps.toFixed(1)} req/s`);
  console.log(`  query latency ms  p50=${pct(latencies, 0.5)}  p95=${pct(latencies, 0.95)}  p99=${pct(latencies, 0.99)}  max=${latencies.length ? latencies[latencies.length - 1].toFixed(1) : 'n/a'}`);
  console.log(`  health latency ms p50=${pct(health, 0.5)}  p95=${pct(health, 0.95)}  max=${health.length ? health[health.length - 1].toFixed(1) : 'n/a'}   (event-loop responsiveness)`);
  console.log(`  errors=${errors.length}${errors.length ? '  e.g. ' + errors[0] : ''}`);
}

main().catch((e) => {
  console.error(e);
  process.exit(1);
});
