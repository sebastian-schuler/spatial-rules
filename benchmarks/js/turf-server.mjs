#!/usr/bin/env bun
// Turf.js HTTP baseline — the memory/throughput comparison for the engine's
// integration server (`integration/server.mjs`).
//
// Wire-compatible with the engine's `/queryRaw`: raw candidate GeoJSON in the
// body, the query as a base64 `x-query` header, raw mask bytes out. That lets
// the same `bun run bench load --endpoint=raw` drive either server.
//
// This is the honest "what an application writes around turf" layer: turf has
// no ruleset model, so the where filter and rule exclusions are plain JS here.
// Supported query surface (the baseline, not the engine):
//   spatial.predicate = intersects only
//   where = implicit-AND equality over rule properties
//   excludeRuleIds
// Anything else is rejected with 400.
//
// Per-request work mirrors a real turf server: parse the candidate GeoJSON into
// feature objects, compute a bbox per candidate, bbox-reject rules, then
// booleanIntersects. Persistent state is the pre-parsed rules + precomputed
// rule bboxes — the same representation `benchmarks/js/memory-turf.mjs` sizes.
//
//   bun benchmarks/js/turf-server.mjs [--port=3000] [--rules-file=...]

import express from 'express';
import { readFileSync } from 'node:fs';
import { feature, booleanIntersects, bbox } from '@turf/turf';

import { readConfig, parseFlags, resolveRepoPath, SPATIAL_QUERY } from '../../shared/config.mjs';

const config = readConfig();
const { values } = parseFlags(process.argv.slice(2), {
  port: { type: 'string' },
  'rules-file': { type: 'string' },
});

const rulesFile = values['rules-file'] ?? resolveRepoPath(config.global.paths.rulesFile);
const rules = JSON.parse(readFileSync(rulesFile, 'utf8')).features;

// Pre-parsed rule representation, held for the process lifetime.
const ruleFeatures = rules.map((r) => feature(r.geometry));
const ruleBboxes = ruleFeatures.map((f) => bbox(f));

function whereMatches(properties, where) {
  for (const [key, value] of Object.entries(where ?? {})) {
    if (value !== null && typeof value === 'object') {
      throw new Error('turf baseline supports equality where only');
    }
    if (properties?.[key] !== value) return false;
  }
  return true;
}

function parseQuery(header) {
  if (!header) return SPATIAL_QUERY;
  return JSON.parse(Buffer.from(String(header), 'base64').toString('utf8'));
}

const app = express();

app.get('/health', (_req, res) => res.json({ ok: true }));

app.post('/queryRaw', express.raw({ type: 'application/octet-stream', limit: '20mb' }), (req, res) => {
  try {
    const query = parseQuery(req.headers['x-query']);
    if ((query.spatial?.predicate ?? 'intersects') !== 'intersects') {
      res.status(400).json({ error: 'turf baseline supports predicate=intersects only' });
      return;
    }

    // Eligible rules are constant for the query — resolve once per request.
    const excluded = new Set(query.excludeRuleIds ?? []);
    const eligible = [];
    for (let r = 0; r < rules.length; r += 1) {
      if (excluded.has(rules[r].id)) continue;
      if (!whereMatches(rules[r].properties, query.where)) continue;
      eligible.push(r);
    }

    const collection = JSON.parse(req.body.toString('utf8'));
    const candidates = collection.features.map((c) => feature(c.geometry));
    const mask = new Array(candidates.length);
    for (let c = 0; c < candidates.length; c += 1) {
      const cb = bbox(candidates[c]);
      let matched = 0;
      for (const r of eligible) {
        const rb = ruleBboxes[r];
        if (cb[0] > rb[2] || cb[2] < rb[0] || cb[1] > rb[3] || cb[3] < rb[1]) continue;
        if (booleanIntersects(candidates[c], ruleFeatures[r])) {
          matched = 1;
          break;
        }
      }
      mask[c] = matched;
    }

    res.setHeader('content-type', 'application/octet-stream');
    res.send(Buffer.from(mask));
  } catch (err) {
    res.status(400).json({ error: err.message });
  }
});

const port = Number(values.port ?? config.global.server.port ?? 3000);
app.listen(port, () => console.log(`turf baseline server listening on ${port}`));
