// Integration server: a small Bun + Express app embedding the addon.
//
//   bun server.mjs   (or: node server.mjs)
//
// Endpoints:
//   GET  /health            -> { ok, stats }
//   POST /query             -> { mask: number[] }  (0 no match, 1 matched, 2 invalid)
//   POST /replace           -> observability (ADR-0007)

import express from 'express';
import { readFileSync } from 'node:fs';
import { dirname, join } from 'node:path';
import { fileURLToPath } from 'node:url';
import { SpatialRuleset } from '../node/index.js';

const here = dirname(fileURLToPath(import.meta.url));
const repoRoot = join(here, '..');

const rulesFile = process.env.RULES_FILE ?? join(repoRoot, 'benchmarks', 'data', 'rules.geojson');
const ruleset = new SpatialRuleset(readFileSync(rulesFile));

const app = express();
app.use(express.json({ limit: '20mb' }));

app.get('/health', (_req, res) => {
  res.json({ ok: true, stats: JSON.parse(ruleset.stats()) });
});

app.post('/query', (req, res) => {
  const { candidates, query } = req.body ?? {};
  if (!candidates) {
    res.status(400).json({ error: 'missing candidates' });
    return;
  }
  const queryJson = JSON.stringify(query ?? { spatial: { predicate: 'intersects' } });
  const mask = ruleset.query(Buffer.from(JSON.stringify(candidates)), queryJson);
  res.json({ mask: Array.from(mask) });
});

// Bytes-in / bytes-out query: raw GeoJSON body (no express.json() object tree
// on the way in), query as a base64 `x-query` header, mask as raw bytes out.
// Models the third-party fetch pattern (no `.json()` call in Node).
app.post('/queryRaw', express.raw({ type: 'application/octet-stream', limit: '20mb' }), (req, res) => {
  const header = req.headers['x-query'];
  const queryJson = header
    ? Buffer.from(String(header), 'base64').toString('utf8')
    : JSON.stringify({ spatial: { predicate: 'intersects' } });
  try {
    const mask = ruleset.query(req.body, queryJson);
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

const port = Number(process.env.PORT ?? 3000);
app.listen(port, () => {
  console.log(`spatial-rules integration server listening on ${port}`);
});
