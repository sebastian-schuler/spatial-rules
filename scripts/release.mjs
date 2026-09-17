#!/usr/bin/env bun
// Manual lockstep release: bump every version source, generate the changelogs
// from conventional commits, commit, tag, and push (which triggers
// prebuild-publish.yml). Replaces release-please.
//
//   bun run release -- 0.3.0            # dry run: prints the plan, writes nothing
//   bun run release -- 0.3.0 --yes      # execute (commit + tag + push)
//
// Flags: --yes (execute), --dry-run (force preview), --no-changelog,
//        --no-push (commit + tag locally, leave pushing to you).
//
// One version across node, wasm, python and the Rust workspace, so a single
// vX.Y.Z tag publishes all three (the publish workflow builds/ships each and
// skips the unchanged ones idempotently).

import { execFileSync } from 'node:child_process';
import { existsSync, readFileSync, writeFileSync } from 'node:fs';
import { dirname, join } from 'node:path';
import { fileURLToPath } from 'node:url';

const ROOT = join(dirname(fileURLToPath(import.meta.url)), '..');
const GIT_CLIFF = 'git-cliff@2.13.1';
const PLATFORMS = [
  'linux-x64-gnu',
  'linux-x64-musl',
  'linux-arm64-gnu',
  'linux-arm64-musl',
  'win32-x64-msvc',
  'win32-arm64-msvc',
];

const argv = process.argv.slice(2);
const flags = new Set(argv.filter((a) => a.startsWith('--')));
const version = argv.find((a) => /^\d+\.\d+\.\d+$/.test(a));
const execute = flags.has('--yes') && !flags.has('--dry-run');
const doChangelog = !flags.has('--no-changelog');
const doPush = execute && !flags.has('--no-push');

const dryRun = !execute;
const log = (msg) => console.log(`${dryRun ? '[dry-run] ' : ''}${msg}`);

function fail(message) {
  console.error(`release: ${message}`);
  process.exit(1);
}

function run(cmd, args, opts = {}) {
  return execFileSync(cmd, args, { cwd: ROOT, encoding: 'utf8', ...opts });
}

const git = (args, opts = {}) => run('git', args, opts);

function read(file) {
  return readFileSync(join(ROOT, file), 'utf8');
}

function write(file, content) {
  if (dryRun) return;
  writeFileSync(join(ROOT, file), content);
}

// --- argument / state guards -------------------------------------------------

if (flags.has('--help') || flags.has('-h')) {
  console.log(
    'usage: bun run release -- <version> [--yes] [--dry-run] [--no-changelog] [--no-push]',
  );
  process.exit(0);
}
if (!version) fail('pass a version, e.g. `bun run release -- 0.3.0`');

const currentNodeVersion = JSON.parse(read('node/package.json')).version;
const cmp = (a, b) => {
  const pa = a.split('.').map(Number);
  const pb = b.split('.').map(Number);
  for (let i = 0; i < 3; i += 1) if (pa[i] !== pb[i]) return pa[i] - pb[i];
  return 0;
};
if (cmp(version, currentNodeVersion) <= 0) {
  fail(`version ${version} must be greater than the current ${currentNodeVersion}`);
}

if (execute && git(['status', '--porcelain']).trim() !== '') {
  fail('working tree is dirty; commit or stash first');
}

const branch = git(['rev-parse', '--abbrev-ref', 'HEAD']).trim();
if (branch !== 'main') {
  console.warn(`release: warning: on '${branch}', not 'main'`);
}

const tag = `v${version}`;
for (const name of [tag, `spatial-rules-wasm-${tag}`, `spatial-rules-python-${tag}`]) {
  try {
    git(['rev-parse', '-q', '--verify', `refs/tags/${name}`]);
    fail(`tag ${name} already exists locally`);
  } catch {
    // not found — good
  }
}
if (git(['ls-remote', '--tags', 'origin', tag]).trim() !== '') {
  fail(`tag ${tag} already exists on origin`);
}

// --- plan --------------------------------------------------------------------

const edits = [];
const edit = (file, content) => {
  edits.push(file);
  write(file, content);
};

// root Cargo.toml — [workspace.package] version
{
  const content = read('Cargo.toml');
  const marker = '[workspace.package]';
  const at = content.indexOf(marker);
  if (at < 0) fail('Cargo.toml has no [workspace.package] section');
  const head = content.slice(0, at);
  const tail = content
    .slice(at)
    .replace(/(version\s*=\s*")[^"]*(")/, `$1${version}$2`);
  edit('Cargo.toml', head + tail);
}

// node/package.json — version + the 6 optionalDependencies
{
  let content = read('node/package.json');
  content = content.replace(/("version"\s*:\s*")[^"]*(")/, `$1${version}$2`);
  content = content.replace(
    /("spatial-rules-(?:win32|linux)-[a-z0-9-]+"\s*:\s*")[^"]*(")/g,
    `$1${version}$2`,
  );
  edit('node/package.json', content);
}

// the 6 platform packages
for (const platform of PLATFORMS) {
  const file = `node/npm/${platform}/package.json`;
  const content = read(file).replace(/("version"\s*:\s*")[^"]*(")/, `$1${version}$2`);
  edit(file, content);
}

// wasm/package.json
{
  const content = read('wasm/package.json').replace(
    /("version"\s*:\s*")[^"]*(")/,
    `$1${version}$2`,
  );
  edit('wasm/package.json', content);
}

log(`version ${currentNodeVersion} -> ${version}`);
log(`files to update:\n  ${edits.join('\n  ')}`);

// --- changelog ---------------------------------------------------------------

const changelog = [];
if (doChangelog) {
  const latestTag = (pattern) => {
    try {
      return git(['describe', '--tags', '--abbrev=0', '--match', pattern, 'HEAD']).trim();
    } catch {
      return null;
    }
  };
  const generate = (matchPattern, includePath) => {
    const prev = latestTag(matchPattern);
    if (!prev) {
      log(`changelog: no previous tag matching '${matchPattern}', skipped`);
      return '';
    }
    const args = ['x', GIT_CLIFF, '-c', 'cliff.toml', '--tag', tag, `${prev}..HEAD`];
    if (includePath) args.push('--include-path', includePath);
    args.push('-o', '-');
    return run(process.execPath, args).trim();
  };
  changelog.push(['CHANGELOG.md', generate('v[0-9]*')]);
  changelog.push([
    'wasm/CHANGELOG.md',
    generate('spatial-rules-wasm-v[0-9]*', 'wasm/**'),
  ]);
  changelog.push([
    'python/CHANGELOG.md',
    generate('spatial-rules-python-v[0-9]*', 'python/**'),
  ]);
  for (const [file, entry] of changelog) {
    if (!entry.includes('## [')) {
      log(`changelog: ${file} has no new entries, skipped`);
      continue;
    }
    const existing = existsSync(join(ROOT, file)) ? read(file) : '';
    // Keep a leading `# Changelog` title at the top; insert under it. Normalise
    // line endings to the file's own (git-cliff emits LF; checkouts may be CRLF).
    const eol = existing.includes('\r\n') ? '\r\n' : '\n';
    const title = existing.match(/^#[^\r\n]*(?:\r?\n)+/);
    const head = title ? title[0] : '';
    const rest = title ? existing.slice(title[0].length) : existing;
    write(file, `${head}${entry}${eol}${eol}${rest}`.replace(/\r?\n/g, eol));
    edits.push(file);
    log(`changelog: prepended to ${file}`);
  }
} else {
  log('changelog: skipped (--no-changelog)');
}

// --- execute -----------------------------------------------------------------

if (dryRun) {
  console.log('\nDry run. Re-run with --yes to bump, commit, tag, and push.');
  process.exit(0);
}

log('refreshing Cargo.lock (cargo update --workspace)');
run('cargo', ['update', '--workspace'], { stdio: 'inherit' });

log(`git add + commit chore(release): ${tag}`);
git(['add', '-A']);
git(['commit', '-m', `chore(release): ${tag}`], { stdio: 'inherit' });

const tags = [tag, `spatial-rules-wasm-${tag}`, `spatial-rules-python-${tag}`];
for (const name of tags) git(['tag', name]);
log(`tagged ${tags.join(', ')}`);

if (doPush) {
  git(['push', 'origin', 'HEAD'], { stdio: 'inherit' });
  git(['push', 'origin', ...tags], { stdio: 'inherit' });
  log(`pushed; the ${tag} tag triggers prebuild-publish.yml`);
} else {
  log('--no-push: commit and tags are local; push them to publish');
}
