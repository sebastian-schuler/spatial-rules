// Server-facing benchmarks: perf, http, load.
//
//   bun server-bench.mjs perf [--iters=3]
//   bun server-bench.mjs http [--iters=10] [--port=3000] [--base-url=http://localhost:3000]
//                            [--rules-file=benchmarks/data/rules.geojson]
//   bun server-bench.mjs load [--endpoint=json|raw] [--concurrency=25] [--duration=10000]
//                             [--base-url=http://localhost:3000]
//
// `http` spawns the integration server itself unless `--base-url` is given;
// `load` expects a server already running (start one with `bun run bench server`).
// All defaults come from benchmarks.json; flags override. Invoked from the repo
// root via `bun run bench <cmd>`.

import { spawn } from 'node:child_process';
import { readFileSync } from 'node:fs';
import { performance } from 'node:perf_hooks';
import { feature, booleanIntersects } from '@turf/turf';
import { REPO_ROOT, sectionConfig, resolveRepoPath, loadNative, matchedCount, SPATIAL_QUERY } from './common.mjs';
import { toTurf, scanMatched } from './turf.mjs';

const { SpatialRuleset } = loadNative();

// The full production query shape every server benchmark drives (spatial +
// `where` + exclusions) — defined once, not per harness.
const PRODUCTION_QUERY = {
  spatial: { predicate: 'intersects' },
  where: { classification: 'restricted' },
  excludeRuleIds: ['rule-00', 'rule-05'],
};

const sub = process.argv[2];
const args = process.argv.slice(3);

switch (sub) {
  case 'perf': runPerf(args); break;
  case 'http': runHttp(args); break;
  case 'load': runLoad(args); break;
  default:
    console.error('usage: bun server-bench.mjs <perf|http|load> [flags]');
    process.exit(2);
}

// === perf — JS performance baseline (turf vs addon, in-process) ============

function runPerf(args) {
  const { cfg, section: perf } = sectionConfig('perf', args);
  const iterations = Number(perf.iters ?? 3);

  const rulesFile = resolveRepoPath(cfg.global.paths.rulesFile);
  const candidatesFile = resolveRepoPath(cfg.global.paths.candidatesFile);

  const rules = JSON.parse(readFileSync(rulesFile, 'utf8')).features;
  const candidates = JSON.parse(readFileSync(candidatesFile, 'utf8')).features;
  const query = JSON.stringify(SPATIAL_QUERY);

  const ruleTurf = toTurf(rules);
  const candidateTurf = toTurf(candidates);

  const candidatesBuffer = Buffer.from(readFileSync(candidatesFile));
  const ruleset = new SpatialRuleset(Buffer.from(readFileSync(rulesFile)));

  // Weakest baseline: naive scan, no index.
  const turfBatch = () => scanMatched(candidateTurf, ruleTurf, { bbox: false });

  const nativeBatch = () => matchedCount(ruleset.query(candidatesBuffer, query));

  function time(label, fn) {
    // Warmup (JIT, lazy init) — not timed.
    fn();
    const samples = [];
    for (let i = 0; i < iterations; i += 1) {
      const start = performance.now();
      const result = fn();
      const ms = performance.now() - start;
      samples.push(ms);
      console.log(`  ${label} run ${i + 1}: ${ms.toFixed(1)} ms (matches=${result})`);
    }
    const mean = samples.reduce((a, b) => a + b, 0) / samples.length;
    return { mean, min: Math.min(...samples) };
  }

  console.log(`workload: ${rules.length} rules × ${candidates.length} candidates (intersects, batch)`);
  console.log('turf.js (baseline A: naive, early-exit on first match):');
  const turf = time('turf', turfBatch);
  console.log('native addon (Buffer → Uint8Array mask):');
  const native = time('native', nativeBatch);

  console.log(`\nturf mean ${turf.mean.toFixed(1)} ms | native mean ${native.mean.toFixed(1)} ms | speedup ${(turf.mean / native.mean).toFixed(1)}×`);
}

// === http — end-to-end production query over HTTP ==========================

async function runHttp(args) {
  const { cfg, section: http, values } = sectionConfig('http', args);
  const port = Number(values.port ?? cfg.global.server.port ?? 3000);
  const iterations = Number(http.iters ?? 10);
  const base = values['base-url'] ?? `http://localhost:${port}`;
  const rulesFile = values['rules-file']
    ? resolveRepoPath(values['rules-file'])
    : resolveRepoPath(cfg.global.paths.rulesFile);
  const candidatesFile = resolveRepoPath(cfg.global.paths.candidatesFile);

  const rulesCollection = JSON.parse(readFileSync(rulesFile, 'utf8'));
  const candidatesCollection = JSON.parse(readFileSync(candidatesFile, 'utf8'));
  const rules = rulesCollection.features;
  const candidates = candidatesCollection.features;
  const ruleFeatures = rules.map((r) => feature(r.geometry));
  const candidateFeatures = candidates.map((c) => feature(c.geometry));

  const query = PRODUCTION_QUERY;

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
  if (!values['base-url']) {
    const serverArgs = ['integration/server.mjs', `--port=${port}`];
    if (values['rules-file']) serverArgs.push(`--rules-file=${rulesFile}`);
    server = spawn('bun', serverArgs, { cwd: REPO_ROOT, stdio: ['ignore', 'ignore', 'ignore'] });
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

  try {
    await waitForHealth();

    // Correctness: the HTTP addon path and the hand-rolled turf path must agree.
    const addonMask = await addonRequest();
    const turfMask = turfFullQuery();
    if (addonMask.length !== turfMask.length || addonMask.some((v, i) => v !== turfMask[i])) {
      throw new Error('mask mismatch between addon and turf');
    }
    const matched = addonMask.filter((v) => v === 1).length;

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

// === load — sustained concurrent load against a running server =============

async function runLoad(args) {
  const { cfg, section: load, values } = sectionConfig('load', args);
  const port = Number(cfg.global.server.port ?? 3000);
  const base = values['base-url'] ?? `http://localhost:${port}`;
  const endpoint = load.endpoint ?? 'json';
  const concurrency = Number(load.concurrency ?? 25);
  const durationMs = Number(load.duration ?? 10_000);

  const candidatesFile = resolveRepoPath(cfg.global.paths.candidatesFile);
  // Raw bytes, read once — the third-party fetch pattern (no `.json()`).
  const rawBody = readFileSync(candidatesFile);
  const query = PRODUCTION_QUERY;
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
  console.log(`endpoint=/${endpoint}${endpoint === 'raw' ? 'Raw' : ''}  concurrency=${concurrency}  duration=${elapsed.toFixed(1)}s`);
  console.log(`  requests=${done}  throughput=${rps.toFixed(1)} req/s`);
  console.log(`  query latency ms  p50=${pct(latencies, 0.5)}  p95=${pct(latencies, 0.95)}  p99=${pct(latencies, 0.99)}  max=${latencies.length ? latencies[latencies.length - 1].toFixed(1) : 'n/a'}`);
  console.log(`  health latency ms p50=${pct(health, 0.5)}  p95=${pct(health, 0.95)}  max=${health.length ? health[health.length - 1].toFixed(1) : 'n/a'}   (event-loop responsiveness)`);
  console.log(`  errors=${errors.length}${errors.length ? '  e.g. ' + errors[0] : ''}`);
}
