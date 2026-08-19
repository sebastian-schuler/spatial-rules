// Neutral config plumbing shared by the benchmark harness and the integration
// app (architecture-hardening 05). Lives outside the benchmark layer so the
// integration app never reaches into the harness for its configuration or
// query shape.
//
// Config lives in one committed file at the repo root — `benchmarks.json` —
// and per-run tweaks come through CLI flags (never environment variables).
// See docs/benchmarks.md for the key -> flag map.

import { readFileSync } from 'node:fs';
import { isAbsolute, join } from 'node:path';
import { dirname } from 'node:path';
import { fileURLToPath } from 'node:url';
import { parseArgs } from 'node:util';

const here = dirname(fileURLToPath(import.meta.url));

// The neutral module lives in shared/, so the repo root is one up.
export const REPO_ROOT = join(here, '..');

// The simple spatial-only query every harness and the app drive — defined once.
export const SPATIAL_QUERY = { spatial: { predicate: 'intersects' } };

// ---- config ---------------------------------------------------------------

export function readConfig() {
  return JSON.parse(readFileSync(join(REPO_ROOT, 'benchmarks.json'), 'utf8'));
}

// Config paths are repo-root-relative; absolute paths (and null) pass through.
export function resolveRepoPath(rel) {
  if (!rel || isAbsolute(rel)) return rel;
  return join(REPO_ROOT, rel);
}

// kebab-case flag (`--rules-file`) -> config key (`rulesFile`).
const toCamel = (key) => key.replace(/-([a-z])/g, (_, c) => c.toUpperCase());

// Parse `--flag=value` (and boolean flags listed in `spec`) with no new deps.
// Unknown flags are collected as strings; per-run tweaks never use env vars.
// A bare `--` separator (e.g. `bun run bench scale -- --sizes=...`) is dropped:
// `bun run` uses it to separate script args from its own flags.
export function parseFlags(args, spec = {}) {
  const { values } = parseArgs({
    args: args.filter((arg) => arg !== '--'),
    options: spec,
    strict: false,
  });
  return { values };
}

// Apply `--flag=value` overrides onto a config section (unknown flags ignored).
export function applyOverrides(section, values) {
  const out = { ...section };
  for (const [key, value] of Object.entries(values)) {
    if (value === undefined) continue;
    out[toCamel(key)] = value;
  }
  return out;
}

// Read one tool's config section with `--flag=value` overrides applied, plus
// the full config (for global paths). The one preamble every harness repeats.
export function sectionConfig(name, args, spec = {}) {
  const cfg = readConfig();
  const { values } = parseFlags(args, spec);
  return { cfg, section: applyOverrides(cfg[name] ?? {}, values), values };
}
