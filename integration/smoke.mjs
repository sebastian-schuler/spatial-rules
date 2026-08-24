// Smoke test for the integration server: posts the production workload
// (30 rules held by the server × 1,000 candidate footprints) and asserts the
// mask shape, then exercises the P1/P2/aggregation surfaces through /query and
// /resolve over a controlled ruleset swapped in via /replace. Run after
// starting the server:
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
const rules = JSON.parse(readFileSync(resolveRepoPath(config.global.paths.rulesFile), 'utf8'));

const health = await fetch(`${base}/health`).then((response) => response.json());
if (!health.ok) throw new Error(`health check failed: ${JSON.stringify(health)}`);

async function postJson(path, body) {
  const response = await fetch(`${base}${path}`, {
    method: 'POST',
    headers: { 'content-type': 'application/json' },
    body: JSON.stringify(body),
  });
  if (!response.ok) throw new Error(`${path} failed: ${response.status}`);
  return response.json();
}

function deepEqual(a, b) {
  if (a === b) return true;
  if (Array.isArray(a) && Array.isArray(b)) {
    return a.length === b.length && a.every((value, index) => deepEqual(value, b[index]));
  }
  if (a && b && typeof a === 'object' && typeof b === 'object') {
    const keysA = Object.keys(a);
    const keysB = Object.keys(b);
    return keysA.length === keysB.length && keysA.every((key) => deepEqual(a[key], b[key]));
  }
  return false;
}

// ---- production workload (the server's default 30-rule ruleset) -----------

const matchMask = (await postJson('/query', { candidates, query: SPATIAL_QUERY })).mask;
if (!Array.isArray(matchMask) || matchMask.length !== candidates.features.length) {
  throw new Error(`expected mask length ${candidates.features.length}, got ${matchMask?.length}`);
}

// /resolve over the same plain query is byte-identical to /query: the
// applicable set and the match set coincide when nothing filters.
const resolveMask = (await postJson('/resolve', { candidates, query: SPATIAL_QUERY })).mask;
if (!deepEqual(resolveMask, matchMask)) {
  throw new Error('resolve mask must match the query mask for the plain query');
}

// withinDistance over the same footprints as points: a 1,000,000 km radius
// admits every point on Earth (the max haversine distance is ~20,000 km), so
// the mask is all-matched and nothing is invalid.
const pointCandidates = {
  type: 'FeatureCollection',
  features: candidates.features.map((feature) => {
    const ring = feature.geometry.coordinates[0];
    const xs = ring.map((coord) => coord[0]);
    const ys = ring.map((coord) => coord[1]);
    return {
      type: 'Feature',
      id: feature.id,
      properties: {},
      geometry: {
        type: 'Point',
        coordinates: [
          (Math.min(...xs) + Math.max(...xs)) / 2,
          (Math.min(...ys) + Math.max(...ys)) / 2,
        ],
      },
    };
  }),
};
const withinMask = (
  await postJson('/query', {
    candidates: pointCandidates,
    query: { spatial: { predicate: 'withinDistance', distance: 1_000_000_000 } },
  })
).mask;
const exceptions = withinMask.filter((value) => value !== 1).length;
if (exceptions > 0) {
  throw new Error(`expected every point within 1,000,000 km, got ${exceptions} exceptions`);
}

// /queryRaw stays byte-oriented and unchanged: raw GeoJSON in, raw mask out,
// byte-identical to /query.
const rawResponse = await fetch(`${base}/queryRaw`, {
  method: 'POST',
  headers: {
    'content-type': 'application/octet-stream',
    'x-query': Buffer.from(JSON.stringify(SPATIAL_QUERY)).toString('base64'),
  },
  body: Buffer.from(JSON.stringify(candidates)),
});
if (!rawResponse.ok) throw new Error(`/queryRaw failed: ${rawResponse.status}`);
const rawMask = Array.from(new Uint8Array(await rawResponse.arrayBuffer()));
if (!deepEqual(rawMask, matchMask)) {
  throw new Error('/queryRaw mask must equal /query mask');
}

// ---- controlled ruleset (exact masks, hand-computed) ----------------------

const controlledRules = {
  type: 'FeatureCollection',
  features: [
    {
      type: 'Feature',
      id: 'zone-a',
      priority: 10,
      properties: {
        active: true,
        name: 'a',
        shared: 'from-a',
        priority: 999,
        daysOfWeek: 1,
        startHour: 0,
        endHour: 24,
      },
      geometry: { type: 'Polygon', coordinates: [[[0, 0], [0, 10], [10, 10], [10, 0], [0, 0]]] },
    },
    {
      type: 'Feature',
      id: 'zone-b',
      priority: 5,
      properties: { active: false, name: 'b', daysOfWeek: 2, startHour: 0, endHour: 24 },
      geometry: { type: 'Polygon', coordinates: [[[100, 100], [100, 110], [110, 110], [110, 100], [100, 100]]] },
    },
    {
      type: 'Feature',
      id: 'zone-c',
      priority: 20,
      properties: { active: true, name: 'c' },
      geometry: { type: 'Polygon', coordinates: [[[2, 2], [2, 12], [12, 12], [12, 2], [2, 2]]] },
    },
  ],
};

const smallCandidates = {
  type: 'FeatureCollection',
  features: [
    { type: 'Feature', id: 'inside', properties: {}, geometry: { type: 'Polygon', coordinates: [[[2, 2], [2, 4], [4, 4], [4, 2], [2, 2]]] } },
    { type: 'Feature', id: 'far', properties: {}, geometry: { type: 'Polygon', coordinates: [[[50, 50], [50, 60], [60, 60], [60, 50], [50, 50]]] } },
    { type: 'Feature', id: 'invalid', properties: {}, geometry: { type: 'Polygon', coordinates: [[[0, 0], [10, 10], [0, 10], [10, 0], [0, 0]]] } },
  ],
};
const pointPair = {
  type: 'FeatureCollection',
  features: [
    { type: 'Feature', id: 'pt-in', properties: {}, geometry: { type: 'Point', coordinates: [5, 5] } },
    { type: 'Feature', id: 'pt-out', properties: {}, geometry: { type: 'Point', coordinates: [50, 50] } },
  ],
};

const replaced = await postJson('/replace', { rules: controlledRules });
if (replaced.version !== 2 || replaced.ruleCount !== 3) {
  throw new Error(`unexpected replace report: ${JSON.stringify(replaced)}`);
}

// withinDistance (ADR-0016): pt-in is inside zone-a (distance 0), pt-out is
// ~5,000 km from every zone — only pt-in admits at 100 m.
const withinResult = await postJson('/query', {
  candidates: pointPair,
  query: { spatial: { predicate: 'withinDistance', distance: 100 } },
});
if (!deepEqual(withinResult.mask, [1, 0])) {
  throw new Error(`expected withinDistance mask [1,0], got ${JSON.stringify(withinResult.mask)}`);
}

// Temporal $activeAt (ADR-0017): zone-a is active Monday 00:00-24:00, zone-b
// Tuesday; zone-c has no window fields and never admits. "inside" reaches
// zone-a (and zone-c). Monday admits it; Tuesday admits only zone-b, which no
// candidate reaches.
const activeAt = { daysOfWeek: 'daysOfWeek', startHour: 'startHour', endHour: 'endHour' };
const monday = await postJson('/query', {
  candidates: smallCandidates,
  query: { spatial: { predicate: 'intersects' }, where: { $activeAt: activeAt }, at: '2026-08-24T10:00' },
});
if (!deepEqual(monday.mask, [1, 0, 2])) {
  throw new Error(`expected temporal Monday mask [1,0,2], got ${JSON.stringify(monday.mask)}`);
}
const tuesday = await postJson('/query', {
  candidates: smallCandidates,
  query: { spatial: { predicate: 'intersects' }, where: { $activeAt: activeAt }, at: '2026-08-25T10:00' },
});
if (!deepEqual(tuesday.mask, [0, 0, 2])) {
  throw new Error(`expected temporal Tuesday mask [0,0,2], got ${JSON.stringify(tuesday.mask)}`);
}

// Aggregation (ADR-0018) rides the rich query path: "inside" matches zone-a +
// zone-c (count 2; the union covers it fully); "far"/"invalid" carry none.
const agg = (
  await postJson('/query', {
    candidates: smallCandidates,
    query: { spatial: { predicate: 'intersects' }, aggregate: { count: true, coverage: true } },
    rich: true,
  })
).outcomes;
if (agg[0].aggregate.count !== 2 || !(agg[0].aggregate.coverage > 0.9)) {
  throw new Error(`unexpected aggregate: ${JSON.stringify(agg[0])}`);
}
if ('aggregate' in agg[1] || 'aggregate' in agg[2]) {
  throw new Error('notMatched/invalid must carry no aggregate');
}

// Resolution (ADR-0015): /resolve returns the compact mask, and the rich form
// the {outcome, winner, values, applicable} explanation. The winner is zone-c
// (top-level priority 20); zone-a gap-fills `shared` and its window fields;
// zone-b is never applicable.
const resolvedMask = (await postJson('/resolve', { candidates: smallCandidates, query: SPATIAL_QUERY })).mask;
if (!deepEqual(resolvedMask, [1, 0, 2])) {
  throw new Error(`expected resolve mask [1,0,2], got ${JSON.stringify(resolvedMask)}`);
}
const resolvedOutcomes = (
  await postJson('/resolve', { candidates: smallCandidates, query: SPATIAL_QUERY, rich: true })
).outcomes;
if (resolvedOutcomes[0].outcome !== 'resolved' || resolvedOutcomes[0].winner !== 'zone-c') {
  throw new Error(`expected winner zone-c, got ${JSON.stringify(resolvedOutcomes[0])}`);
}
if (!deepEqual(resolvedOutcomes[0].values, {
  active: true,
  name: 'c',
  priority: 999,
  shared: 'from-a',
  daysOfWeek: 1,
  startHour: 0,
  endHour: 24,
})) {
  throw new Error(`unexpected resolved values: ${JSON.stringify(resolvedOutcomes[0].values)}`);
}
if (!deepEqual(resolvedOutcomes[0].applicable, [
  { ruleId: 'zone-c', priority: 20, spatialMatched: true, propertyMatched: true },
  { ruleId: 'zone-a', priority: 10, spatialMatched: true, propertyMatched: true },
])) {
  throw new Error(`unexpected applicable set: ${JSON.stringify(resolvedOutcomes[0].applicable)}`);
}
if (resolvedOutcomes[1].outcome !== 'notMatched' || resolvedOutcomes[2].outcome !== 'invalid') {
  throw new Error(`unexpected outcomes: ${JSON.stringify(resolvedOutcomes.slice(1))}`);
}

// Aggregation rides the resolution rich path too.
const resolveAgg = (
  await postJson('/resolve', {
    candidates: smallCandidates,
    query: { spatial: { predicate: 'intersects' }, aggregate: { count: true } },
    rich: true,
  })
).outcomes;
if (resolveAgg[0].aggregate.count !== 2 || 'aggregate' in resolveAgg[1]) {
  throw new Error(`unexpected resolution aggregate: ${JSON.stringify(resolveAgg)}`);
}

// Restore the production ruleset so a long-lived server stays in its original
// state for the harnesses that follow.
const restored = await postJson('/replace', { rules });
if (restored.ruleCount !== 30) {
  throw new Error(`restore failed: ${JSON.stringify(restored)}`);
}

const matched = matchMask.filter((value) => value === 1).length;
console.log(`smoke: ${matchMask.length} candidates, ${matched} matched`);
console.log('integration smoke passed');
