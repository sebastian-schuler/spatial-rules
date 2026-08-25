// Integration server: a small Bun + Express app embedding the addon.
//
//   bun server.mjs [--port=3000] [--rules-file=benchmarks/data/rules.geojson]
//
// Defaults come from the repo-root benchmarks.json; flags override. Start it
// from the repo root with `bun run bench server`.
//
// Endpoints:
//   GET  /health            -> { ok, stats }
//   POST /query             -> { mask: number[] }  (0 no match, 1 matched, 2 invalid)
//                             with `rich: true` also { outcomes } — the per-candidate
//                             match outcomes, carrying the query's `aggregate` /
//                             `includeOverlap` payloads
//   POST /resolve           -> { mask: number[] }  (0 no resolution, 1 resolved, 2 invalid)
//                             with `rich: true` also { outcomes } — the per-candidate
//                             {outcome, winner, values, applicable, aggregate} (ADR-0015)
//   POST /replace           -> observability (ADR-0007)

import express from 'express';
import { readFileSync } from 'node:fs';
import { SpatialRuleset } from '../node/index.ts';
import { readConfig, parseFlags, resolveRepoPath, SPATIAL_QUERY } from '../shared/config.mjs';

const config = readConfig();

// CLI overrides only — the harness never uses environment variables.
const { values } = parseFlags(process.argv.slice(2), {
  port: { type: 'string' },
  'rules-file': { type: 'string' },
});

const rulesFile = values['rules-file'] ?? resolveRepoPath(config.global.paths.rulesFile);
const ruleset = new SpatialRuleset(readFileSync(rulesFile));

const app = express();
app.use(express.json({ limit: '20mb' }));

app.get('/health', (_req, res) => {
  res.json({ ok: true, stats: JSON.parse(ruleset.stats()) });
});

app.post('/query', (req, res) => {
  const { candidates, query, rich } = req.body ?? {};
  if (!candidates) {
    res.status(400).json({ error: 'missing candidates' });
    return;
  }
  const queryJson = JSON.stringify(query ?? SPATIAL_QUERY);
  try {
    const result = ruleset.query(Buffer.from(JSON.stringify(candidates)), queryJson);
    const body = { mask: Array.from(result.mask()) };
    if (rich) body.outcomes = JSON.parse(result.toOutcomesJson());
    res.json(body);
  } catch (err) {
    res.status(400).json({ error: err.message, code: err.code });
  }
});

// The resolution surface (ADR-0015): the compact mask by default; with
// `rich: true`, the per-candidate {outcome, winner, values, applicable}
// explanation (plus the query's aggregate, ADR-0018). Same request shape as
// /query — the query object's `spatial`/`where`/`at`/`aggregate` members parse
// through the same `Query` path.
app.post('/resolve', (req, res) => {
  const { candidates, query, rich } = req.body ?? {};
  if (!candidates) {
    res.status(400).json({ error: 'missing candidates' });
    return;
  }
  const queryJson = JSON.stringify(query ?? SPATIAL_QUERY);
  try {
    const result = ruleset.resolve(Buffer.from(JSON.stringify(candidates)), queryJson);
    const body = { mask: Array.from(result.mask()) };
    if (rich) body.outcomes = JSON.parse(result.toJson());
    res.json(body);
  } catch (err) {
    res.status(400).json({ error: err.message, code: err.code });
  }
});

// Bytes-in / bytes-out query: raw GeoJSON body (no express.json() object tree
// on the way in), query as a base64 `x-query` header, mask as raw bytes out.
// Models the third-party fetch pattern (no `.json()` call in Node).
app.post('/queryRaw', express.raw({ type: 'application/octet-stream', limit: '20mb' }), (req, res) => {
  const header = req.headers['x-query'];
  const queryJson = header
    ? Buffer.from(String(header), 'base64').toString('utf8')
    : JSON.stringify(SPATIAL_QUERY);
  try {
    const mask = ruleset.query(req.body, queryJson).mask();
    res.setHeader('content-type', 'application/octet-stream');
    res.send(Buffer.from(mask));
  } catch (err) {
    res.status(400).json({ error: err.message, code: err.code });
  }
});

app.post('/replace', (req, res) => {
  const { rules } = req.body ?? {};
  if (!rules) {
    res.status(400).json({ error: 'missing rules' });
    return;
  }
  try {
    const report = JSON.parse(ruleset.replace(Buffer.from(JSON.stringify(rules))));
    res.json(report);
  } catch (err) {
    res.status(400).json({ error: err.message, code: err.code });
  }
});

const port = Number(values.port ?? config.global.server.port ?? 3000);
app.listen(port, () => {
  console.log(`spatial-rules integration server listening on ${port}`);
});

