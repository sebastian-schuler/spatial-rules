#!/usr/bin/env bun
// One entry point for the benchmark + integration harness.
//
//   bun run bench <cmd> [flags]      (or: bun bench.mjs <cmd> [flags])
//
// Commands:
//   build        build the native binding (+ copy to node/spatial_rules.node) and the cross_check binary
//   gen          regenerate the synthetic dataset (benchmarks/data/*.geojson)
//   data         download Natural Earth countries.geojson + derive deu.geojson (real-data mode)
//   cross-check  turf vs Rust DE-9IM predicate cross-check (needs `build`)
//   scale|fair|complex|crossover   sweep/experiment harnesses
//   perf|http|load                 server-facing benchmarks
//   server       start the integration server
//   smoke        integration smoke (server must be running)
//   memory       container memory harness [--replacements-only]
//   memory-scale memory scaling & lifecycle benchmark (rules × vertices grid)
//   memory-turf  engine vs turf.js memory footprint (same synthetic rules)
//   python       engine (PyO3 wheel) vs Shapely/GEOS baseline [--reps= --points= --candidates= --rules-file=]
//   smoke:node   node package smoke test
//   crit         criterion algorithm ladder
//   all          full battery (build + gen if needed; then cross-check/scale/fair/complex/crossover/perf/http/memory)
//
// No environment variables anywhere — every knob lives in benchmarks.json and
// is overridable per-run with `--flag=value`.

import { spawnSync } from 'node:child_process';
import { copyFileSync, existsSync, mkdirSync, readFileSync, writeFileSync } from 'node:fs';
import { dirname, join } from 'node:path';
import { fileURLToPath } from 'node:url';

const REPO_ROOT = dirname(fileURLToPath(import.meta.url));
const CONFIG = JSON.parse(readFileSync(join(REPO_ROOT, 'benchmarks.json'), 'utf8'));

const NODE_BINDING = join(REPO_ROOT, CONFIG.global.paths.nodeBinding);
const CROSS_CHECK_BIN = join(REPO_ROOT, CONFIG.global.paths.crossCheckBin);
const RULES_FILE = join(REPO_ROOT, CONFIG.global.paths.rulesFile);
const CANDIDATES_FILE = join(REPO_ROOT, CONFIG.global.paths.candidatesFile);
const COUNTRIES_FILE = join(REPO_ROOT, CONFIG.global.paths.realRulesFile);
// deu.geojson lives beside countries.geojson — derived from the config path.
const DEU_FILE = join(dirname(COUNTRIES_FILE), 'deu.geojson');

const NE_URL =
  'https://raw.githubusercontent.com/nvkelso/natural-earth-vector/master/geojson/ne_10m_admin_0_countries.geojson';

const SWEEPS = join(REPO_ROOT, 'benchmarks', 'js', 'sweeps.mjs');
const SERVER_BENCH = join(REPO_ROOT, 'benchmarks', 'js', 'server-bench.mjs');
const CROSS_CHECK = join(REPO_ROOT, 'benchmarks', 'js', 'cross_check.mjs');
const MEMORY = join(REPO_ROOT, 'integration', 'memory.mjs');
const MEMORY_SCALE_BIN = join(REPO_ROOT, 'target', 'release', `memory_scaling${process.platform === 'win32' ? '.exe' : ''}`);
const MEMORY_TURF = join(REPO_ROOT, 'benchmarks', 'js', 'memory-turf.mjs');
const PY_BENCH = join(REPO_ROOT, 'benchmarks', 'py', 'bench.py');
const PY_VENV_PYTHON = join(REPO_ROOT, 'python', '.venv', process.platform === 'win32' ? 'Scripts\\python.exe' : 'bin/python');
const SERVER = join(REPO_ROOT, 'integration', 'server.mjs');
const SMOKE = join(REPO_ROOT, 'integration', 'smoke.mjs');
const NODE_SMOKE = join(REPO_ROOT, 'node', 'test', 'smoke.ts');

// Run a command synchronously with output inherited; exit on failure.
function run(cmd, args, opts = {}) {
  const result = spawnSync(cmd, args, { cwd: REPO_ROOT, stdio: 'inherit', ...opts });
  if (result.status !== 0) process.exit(result.status ?? 1);
}

// ---- prerequisites --------------------------------------------------------

function ensureNodeBinding() {
  if (!existsSync(NODE_BINDING)) {
    console.error(`missing native binding at ${NODE_BINDING} — run \`bun run bench build\` first`);
    process.exit(1);
  }
}

function ensureCrossCheck() {
  if (!existsSync(CROSS_CHECK_BIN) && !existsSync(`${CROSS_CHECK_BIN}.exe`)) {
    console.error(`missing cross_check binary — run \`bun run bench build\` first`);
    process.exit(1);
  }
}

function ensureDataset() {
  if (!existsSync(RULES_FILE) || !existsSync(CANDIDATES_FILE)) {
    console.error(`missing dataset — run \`bun run bench gen\` first`);
    process.exit(1);
  }
}

// ---- commands -------------------------------------------------------------

function cmdBuild() {
  console.log('building native binding (release)...');
  run('cargo', ['build', '--release', '-p', 'spatial-rules-node']);

  const libName =
    process.platform === 'win32'
      ? 'spatial_rules_node.dll'
      : process.platform === 'darwin'
        ? 'libspatial_rules_node.dylib'
        : 'libspatial_rules_node.so';
  const built = join(REPO_ROOT, 'target', 'release', libName);
  if (!existsSync(built)) {
    console.error(`expected built binding at ${built} — not found`);
    process.exit(1);
  }
  copyFileSync(built, NODE_BINDING);
  console.log(`copied ${built} -> ${NODE_BINDING}`);

  console.log('building cross_check binary (release)...');
  run('cargo', ['build', '--release', '-p', 'spatial-rules-benchmarks', '--bin', 'cross_check']);
  console.log('build complete');
}

function cmdGen() {
  run('cargo', ['run', '-p', 'spatial-rules-benchmarks', '--bin', 'generate_dataset']);
}

async function cmdData() {
  mkdirSync(dirname(COUNTRIES_FILE), { recursive: true });
  if (!existsSync(COUNTRIES_FILE)) {
    console.log(`downloading Natural Earth countries -> ${COUNTRIES_FILE}`);
    const response = await fetch(NE_URL);
    if (!response.ok) {
      console.error(`download failed: HTTP ${response.status}`);
      process.exit(1);
    }
    const text = await response.text();
    writeFileSync(COUNTRIES_FILE, text);
    console.log(`wrote ${COUNTRIES_FILE} (${(text.length / 1024 / 1024).toFixed(1)} MiB)`);
  } else {
    console.log(`${COUNTRIES_FILE} already present`);
  }

  // Derive a focused single-country file (Germany) from the full set.
  const countries = JSON.parse(readFileSync(COUNTRIES_FILE, 'utf8'));
  const deu = {
    type: 'FeatureCollection',
    features: countries.features.filter((f) => f.properties?.ADMIN === 'Germany'),
  };
  writeFileSync(DEU_FILE, JSON.stringify(deu));
  console.log(`wrote ${DEU_FILE} (${deu.features.length} feature(s))`);

  console.log('\nreal-data mode (paths are repo-relative):');
  console.log(`  bun run bench complex --rules-file=${CONFIG.global.paths.realRulesFile}`);
  console.log(`  bun run bench crossover --rules-file=${CONFIG.global.paths.realRulesFile}`);
  console.log('focused run:');
  console.log(`  bun run bench complex --rules-file=benchmarks/data/deu.geojson`);
}

function cmdCrit(args) {
  run('cargo', ['bench', '-p', 'spatial-rules-benchmarks', '--bench', 'ladder', ...args]);
}

async function cmdAll() {
  if (!existsSync(NODE_BINDING)) {
    console.log('binding missing — running `bun run bench build` first\n');
    cmdBuild();
  }
  if (!existsSync(RULES_FILE) || !existsSync(CANDIDATES_FILE)) {
    console.log('dataset missing — running `bun run bench gen` first\n');
    cmdGen();
  }
  ensureCrossCheck();

  const battery = [
    { name: 'cross-check', file: CROSS_CHECK, needsSub: false },
    { name: 'scale', file: SWEEPS, needsSub: true },
    { name: 'fair', file: SWEEPS, needsSub: true },
    { name: 'complex', file: SWEEPS, needsSub: true },
    { name: 'crossover', file: SWEEPS, needsSub: true },
    { name: 'perf', file: SERVER_BENCH, needsSub: true },
    { name: 'http', file: SERVER_BENCH, needsSub: true },
    { name: 'memory', file: MEMORY, needsSub: false },
    { name: 'python', file: PY_BENCH, needsSub: false },
  ];
  for (const { name, file, needsSub } of battery) {
    console.log(`\n=== bun run bench ${name} ===`);
    if (name === 'python') cmdPython([]);
    else run('bun', needsSub ? [file, name] : [file]);
  }
  console.log('\n`load` (concurrency) needs a running server — `bun run bench server`, then `bun run bench load`.');
}

function cmdPython(args) {
  // Build the PyO3 wheel into the dev venv if not already importable.
  const probe = spawnSync(PY_VENV_PYTHON, ['-c', 'import spatial_rules'], { cwd: REPO_ROOT, stdio: 'pipe' });
  if (probe.status !== 0) {
    console.log('spatial-rules not importable in python/.venv — building the wheel (maturin develop --release)...');
    run(PY_VENV_PYTHON, ['-m', 'maturin', 'develop', '--release'], { cwd: join(REPO_ROOT, 'python') });
  } else {
    console.log('spatial-rules already importable — skipping wheel build');
  }

  // Ensure the Shapely/GEOS baseline dependencies are present.
  const deps = spawnSync(PY_VENV_PYTHON, ['-c', 'import shapely, numpy'], { cwd: REPO_ROOT, stdio: 'pipe' });
  if (deps.status !== 0) {
    console.log('installing shapely + numpy into python/.venv...');
    run(PY_VENV_PYTHON, ['-m', 'pip', 'install', '--quiet', 'shapely>=2.0', 'numpy']);
  }

  if (!existsSync(RULES_FILE) || !existsSync(CANDIDATES_FILE)) {
    console.error(`missing dataset — run \`bun run bench gen\` first`);
    process.exit(1);
  }
  run(PY_VENV_PYTHON, [PY_BENCH, ...args]);
}

function cmdMemoryScale(args) {
  if (!existsSync(MEMORY_SCALE_BIN)) {
    console.log('memory_scaling binary missing — building (release)...');
    run('cargo', ['build', '--release', '-p', 'spatial-rules-benchmarks', '--bin', 'memory_scaling']);
  }
  const section = CONFIG.memoryScale ?? {};
  // Explicit cells win over the rules × vertices cross product. `--cells` is
  // the default grid (bounded so a default run always finishes); pass
  // `--rules= --vertices=` to run a full cross product instead.
  const overridden = new Set(args.map((arg) => arg.split('=')[0]));
  const defaults = [];
  if (
    section.cells &&
    !overridden.has('--cells') &&
    !overridden.has('--rules') &&
    !overridden.has('--vertices')
  ) {
    defaults.push(`--cells=${section.cells}`);
  } else {
    defaults.push(`--rules=${section.rules ?? '1000,10000,100000'}`);
    defaults.push(`--vertices=${section.vertices ?? '10,100,1000'}`);
  }
  for (const [key, value] of [
    ['--candidates', section.candidates ?? 1000],
    ['--query-batches', section.queryBatches ?? 20],
    ['--replacements', section.replacements ?? 20],
  ]) {
    if (!overridden.has(key)) defaults.push(`${key}=${value}`);
  }
  run(MEMORY_SCALE_BIN, [...defaults, ...args]);
}

function printHelp() {
  console.log(`spatial-rules benchmark & integration harness

usage: bun run bench <cmd> [flags]

  build         build native binding (+ copy to node/spatial_rules.node) + cross_check binary
  gen           regenerate the synthetic dataset (benchmarks/data/*.geojson)
  data          download Natural Earth countries.geojson + derive deu.geojson (real-data mode)
  cross-check   turf vs Rust DE-9IM predicate cross-check
  scale         scaling sweep (turf vs addon as the workload grows)
  fair          fair competitor: rbush + turf vs the addon
  complex       complexity & metadata stress
  crossover     candidate/rule-count break-even sweep
  perf          JS performance baseline (turf vs addon, in-process)
  http          full production query over HTTP (spawns the server)
  load          sustained concurrent load (server must be running)
  server        start the integration server
  smoke         integration smoke (server must be running)
  memory        container memory harness  [--replacements-only]
  memory-scale  memory scaling & lifecycle benchmark [--cells= --rules= --vertices= --candidates= --query-batches= --replacements=]
  memory-turf   engine vs turf.js memory footprint, same synthetic rules [--cells=]
  python        engine (PyO3 wheel) vs Shapely/GEOS baseline [--reps= --points= --candidates= --rules-file=]
  smoke:node    node package smoke test
  crit          criterion algorithm ladder (cargo bench)
  all           full battery (build + gen if needed)
  help          this list

Every knob defaults to benchmarks.json and can be overridden per run, e.g.
  bun run bench crossover --sizes=20,200,1000,5000 --reps=5
  bun run bench complex --rules-file=benchmarks/data/countries.geojson
  bun run bench memory --replacements-only`);
}

// ---- dispatch -------------------------------------------------------------

const [cmd, ...args] = process.argv.slice(2);

switch (cmd) {
  case 'build': cmdBuild(); break;
  case 'gen': cmdGen(); break;
  case 'data': await cmdData(); break;
  case 'cross-check': ensureCrossCheck(); run('bun', [CROSS_CHECK, ...args]); break;
  case 'scale':
  case 'fair':
  case 'complex':
  case 'crossover':
    ensureNodeBinding();
    run('bun', [SWEEPS, cmd, ...args]);
    break;
  case 'perf':
  case 'http':
  case 'load':
    ensureNodeBinding();
    run('bun', [SERVER_BENCH, cmd, ...args]);
    break;
  case 'server': ensureNodeBinding(); run('bun', [SERVER, ...args]); break;
  case 'smoke': run('bun', [SMOKE, ...args]); break;
  case 'memory': ensureNodeBinding(); run('bun', [MEMORY, ...args]); break;
  case 'memory-scale': cmdMemoryScale(args); break;
  case 'memory-turf': run('bun', [MEMORY_TURF, ...args]); break;
  case 'python': cmdPython(args); break;
  case 'smoke:node': ensureNodeBinding(); run('bun', [NODE_SMOKE, ...args]); break;
  case 'crit': cmdCrit(args); break;
  case 'all': await cmdAll(); break;
  case undefined:
  case 'help':
  case '-h':
  case '--help':
    printHelp();
    break;
  default:
    console.error(`unknown command: ${cmd}\n`);
    printHelp();
    process.exit(2);
}
