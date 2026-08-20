// Memory harness — measures the process footprint (RSS + Linux VmHWM/VmPeak) of
// the Bun + addon + 30-rule ruleset under (a) query load and (b) ruleset
// replacement. Closes the deferred follow-up from tickets 17/19: §25 requires
// "peak memory during replacement MUST be measured because the application runs
// in constrained containers".
//
//   bun memory.mjs [--query-batches=20] [--replacements=10] [--replacements-only]
//                  [--rules-file=benchmarks/data/rules.geojson]
//                  [--candidates-file=benchmarks/data/candidates.geojson]
//
// Defaults come from the repo-root benchmarks.json; flags override. Run from
// the repo root via `bun run bench memory`. Works locally (Windows: no
// /proc/self/status, so only RSS samples) and inside the Docker image
// (VmHWM/VmPeak give the exact high-water mark).
//
// Metric notes:
//   - VmHWM (/proc/self/status) is the exact all-time peak RSS, recorded by the
//     kernel — it captures the moment during replace() when the old ruleset and
//     the in-build new one coexist, even though the single-threaded event loop
//     cannot sample mid-call. This is the authoritative §25 number.
//   - Sampled RSS is a lower-bound cross-check (point-in-time).
//   - The boundedness check: RSS after the first vs the last replacement. If
//     memory were leaked per replacement, last ≫ first; if bounded, they agree.

import { readFileSync } from 'node:fs';
import { SpatialRuleset } from '../node/index.ts';
import { readConfig, parseFlags, resolveRepoPath, SPATIAL_QUERY } from '../shared/config.mjs';

const config = readConfig();

// CLI overrides only — the harness never uses environment variables.
const { values } = parseFlags(process.argv.slice(2), {
  'rules-file': { type: 'string' },
  'candidates-file': { type: 'string' },
  'query-batches': { type: 'string' },
  replacements: { type: 'string' },
  'replacements-only': { type: 'boolean' },
});

const rulesFile = values['rules-file'] ?? resolveRepoPath(config.global.paths.rulesFile);
const candidatesFile =
  values['candidates-file'] ?? resolveRepoPath(config.global.paths.candidatesFile);

const QUERY_BATCHES = Number(values['query-batches'] ?? config.memory.queryBatches ?? 20);
const REPLACEMENTS = Number(values.replacements ?? config.memory.replacements ?? 10);
// Flag overrides the config default; the knob exists in benchmarks.json too.
const REPLACEMENTS_ONLY = Boolean(values['replacements-only'] ?? config.memory.replacementsOnly);

const rulesBuf = readFileSync(rulesFile);
const candidates = JSON.parse(readFileSync(candidatesFile, 'utf8'));
const queryJson = JSON.stringify(SPATIAL_QUERY);

const mb = (bytes) => (bytes == null ? null : Number((bytes / 1024 / 1024).toFixed(1)));

// Linux-only exact counters from /proc/self/status (kB → bytes). Null elsewhere.
function vm() {
  try {
    const status = readFileSync('/proc/self/status', 'utf8');
    const field = (name) => {
      const match = status.match(new RegExp(`^${name}:\\s+(\\d+)`, 'm'));
      return match ? Number(match[1]) * 1024 : null;
    };
    return { vmPeak: field('VmPeak'), vmHwm: field('VmHWM'), vmRss: field('VmRSS'), vmData: field('VmData') };
  } catch {
    return null;
  }
}

const sample = () => {
  const v = vm();
  return {
    rss: process.memoryUsage().rss,
    vmPeak: v?.vmPeak ?? null,
    vmHwm: v?.vmHwm ?? null,
    vmRss: v?.vmRss ?? null,
    vmData: v?.vmData ?? null,
  };
};

const report = {
  workload: {
    candidatesPerBatch: candidates.features.length,
    queryBatches: QUERY_BATCHES,
    replacements: REPLACEMENTS,
    replacementsOnly: REPLACEMENTS_ONLY,
  },
  phases: {},
};

// Phase 0: build the initial ruleset (baseline footprint).
const ruleset = new SpatialRuleset(rulesBuf);
report.phases.afterBuild = { rss: mb(sample().rss), vmHwm: mb(sample().vmHwm), vmPeak: mb(sample().vmPeak) };

if (!REPLACEMENTS_ONLY) {
  // Phase 1: query load — QUERY_BATCHES batches of 1,000 candidates.
  let rssSamples = [];
  for (let i = 0; i < QUERY_BATCHES; i++) {
    ruleset.query(Buffer.from(JSON.stringify(candidates)), queryJson);
    if (i % 5 === 0) rssSamples.push(process.memoryUsage().rss);
  }
  report.phases.afterQueries = {
    peakSampledRss: mb(Math.max(...rssSamples)),
    vmHwm: mb(sample().vmHwm),
    vmPeak: mb(sample().vmPeak),
  };
}

// Phase 2: replacement — build a fresh 30-rule ruleset and swap, REPLACEMENTS
// times. Each swap drops the previous ruleset; VmHWM records the true peak
// (old + in-build new coexist) even though sampling can't fire mid-call.
const rssAfterEachReplace = [];
for (let i = 0; i < REPLACEMENTS; i++) {
  ruleset.replace(rulesBuf);
  rssAfterEachReplace.push(process.memoryUsage().rss);
}
report.phases.afterReplacements = {
  rssFirst: mb(rssAfterEachReplace[0]),
  rssLast: mb(rssAfterEachReplace.at(-1)),
  peakSampledRss: mb(Math.max(...rssAfterEachReplace)),
  vmHwm: mb(sample().vmHwm),
  vmPeak: mb(sample().vmPeak),
};

// Boundedness: RSS must not climb across replacements (leak ⇒ last ≫ first).
report.phases.boundedness = {
  rssSpreadAcrossReplacementsMb: mb(rssAfterEachReplace.at(-1) - rssAfterEachReplace[0]),
  note: '≈0 means no per-replacement leak; a climb would flag one',
};

console.log(JSON.stringify(report, null, 2));
