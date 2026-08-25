#!/usr/bin/env bun
// Memory footprint comparison — Rust engine vs the turf.js baseline, on the
// same synthetic rules. Answers "what does it cost to *hold* a ruleset" in
// each stack:
//
//   engine ruleset   the indexed/compiled ruleset (memory-scale steady-state)
//   engine serving   ruleset + per-thread prepared-geometry memo (post-query;
//                    lazy, so ~ruleset at the default 1,000 candidates)
//   turf rss         the pre-parsed feature objects + precomputed bboxes the
//                    timed turf baseline holds (fresh-process RSS delta)
//
//   bun memory-turf.mjs [--cells=1000x10,1000x100,...]
//
// Defaults come from `benchmarks.json` `memoryScale`. Each cell runs the turf
// measurement in a **fresh child process** (self re-exec via `--single=r,v`),
// exactly like the Rust `memory_scaling` harness, so every cell sees a clean
// baseline and there is no allocator carryover between cells. The turf side is
// sampled as a post-forced-GC RSS delta — process-level ground truth, the same
// philosophy as `docs/benchmarks.md` §Memory. (Bun's `heapUsed` under-reports
// nested coordinate arrays, so RSS is used rather than the JS heap.)

import { spawnSync } from 'node:child_process';
import { existsSync } from 'node:fs';
import { fileURLToPath } from 'node:url';
import { join } from 'node:path';
import { sectionConfig, makeRng, REPO_ROOT } from './common.mjs';

const EXE = join(
  REPO_ROOT,
  'target',
  'release',
  `memory_scaling${process.platform === 'win32' ? '.exe' : ''}`,
);
const SCRIPT = fileURLToPath(import.meta.url);

const mb = (bytes) => (bytes == null ? '—' : (bytes / 1024 / 1024).toFixed(1));

// "1000x10,10000x100" -> [{ rules, vertices }]
function parseCells(text) {
  return text
    .split(',')
    .map((part) => {
      const [rules, vertices] = part.trim().split('x').map(Number);
      return { rules, vertices };
    })
    .filter((cell) => Number.isFinite(cell.rules) && Number.isFinite(cell.vertices));
}

// Star-shaped ring with exactly `vertices` distinct points + closing repeat —
// same shape class as the Rust generator (positive radius everywhere => simple
// => passes strict validation), same per-ring complexity.
function ring(rng, cx, cy, vertices) {
  const coords = [];
  for (let i = 0; i < vertices; i += 1) {
    const angle = (i / vertices) * Math.PI * 2;
    const r = 1.0 + 0.45 * rng();
    coords.push([cx + r * Math.cos(angle), cy + r * Math.sin(angle)]);
  }
  coords.push(coords[0]);
  return coords;
}

// The pre-parsed representation the timed turf baseline holds: feature objects
// plus a precomputed per-rule bbox (the "bbox fast-reject" baseline), all kept
// alive so GC cannot reclaim them mid-measurement.
function buildTurfRepresentation(keep, rules, vertices) {
  const rng = makeRng(0x5eed_1a2b_3c4d);
  const columns = Math.ceil(Math.sqrt(rules));
  const pitch = 10;
  for (let i = 0; i < rules; i += 1) {
    const cx = (i % columns) * pitch;
    const cy = Math.floor(i / columns) * pitch;
    const coords = ring(rng, cx, cy, vertices);
    let minX = Infinity;
    let minY = Infinity;
    let maxX = -Infinity;
    let maxY = -Infinity;
    for (const [x, y] of coords) {
      if (x < minX) minX = x;
      if (x > maxX) maxX = x;
      if (y < minY) minY = y;
      if (y > maxY) maxY = y;
    }
    keep.push({
      type: 'Feature',
      id: `rule-${i}`,
      properties: { classification: `c${i % 5}` },
      geometry: { type: 'Polygon', coordinates: [coords] },
    });
    keep.push([minX, minY, maxX, maxY]);
  }
}

function forceGc() {
  if (typeof Bun !== 'undefined' && typeof Bun.gc === 'function') Bun.gc(true);
  else if (typeof global.gc === 'function') global.gc();
}

// ---- child mode: measure one cell's turf footprint in this process ---------

function childMeasure(rules, vertices) {
  globalThis.__keep = [];
  forceGc();
  const base = process.memoryUsage();
  buildTurfRepresentation(globalThis.__keep, rules, vertices);
  forceGc();
  const after = process.memoryUsage();
  console.log(JSON.stringify({ rss: after.rss - base.rss }));
}

// ---- engine side: run the release memory_scaling binary for one cell --------

function engineFootprint(rules, vertices) {
  const { status, stdout, stderr } = spawnSync(
    EXE,
    [
      `--cell=${rules},${vertices}`,
      '--replacements=1',
      '--query-batches=1',
      '--candidates=1000',
    ],
    { encoding: 'utf8', stdio: ['ignore', 'pipe', 'inherit'] },
  );
  if (status !== 0) {
    console.error(stderr || `memory_scaling exited with ${status}`);
    process.exit(status ?? 1);
  }
  const report = JSON.parse(stdout);
  return {
    ruleset: report.steady_state_delta_bytes,
    serving: report.query_time.rss_first_bytes - report.baseline_rss_bytes,
  };
}

// ---- dispatch ---------------------------------------------------------------

const args = process.argv.slice(2);
const single = args.find((arg) => arg.startsWith('--single='));
if (single) {
  const [rules, vertices] = single.split('=')[1].split('x').map(Number);
  childMeasure(rules, vertices);
  process.exit(0);
}

const { section } = sectionConfig('memoryScale', args);
const cells =
  parseCells(section.cells) ||
  parseCells('1000x10,1000x100,1000x1000,10000x10,10000x100,100000x10,100000x100');

if (!existsSync(EXE)) {
  console.error(`missing ${EXE} — build it with \`bun run bench memory-scale\` first`);
  process.exit(1);
}

console.log('rules x verts | engine ruleset | engine serving* | turf rss | turf kB/rule');
console.log('             |                | (+prepared memo)|          |     (rss)');
for (const { rules, vertices } of cells) {
  const engine = engineFootprint(rules, vertices);
  const { status, stdout, stderr } = spawnSync(
    process.execPath,
    [SCRIPT, `--single=${rules}x${vertices}`],
    { encoding: 'utf8' },
  );
  if (status !== 0) {
    console.error(stderr || `turf measurement child exited with ${status}`);
    process.exit(status ?? 1);
  }
  const turf = JSON.parse(stdout);
  const label = `${rules} x ${vertices}`.padEnd(13);
  console.log(
    `${label} | ${mb(engine.ruleset).padStart(9)} MiB | ${mb(engine.serving).padStart(9)} MiB | ` +
      `${mb(turf.rss).padStart(7)} MiB | ${(turf.rss / rules / 1024).toFixed(1)} kB`,
  );
}
console.log('\n* serving = ruleset + per-thread prepared-geometry memo (ADR-0010), lazily prepared per touched rule, after the first query.');