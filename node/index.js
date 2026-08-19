// Thin JS wrapper over the native addon (ADR-0005, ADR-0006).
//
// Defines `SpatialRulesError extends Error` with a `.code` property, and
// re-throws native errors (which carry `SR_*` codes) as that class.
//
// The native binary resolves from the per-platform optionalDependency package
// (`spatial-rules-<triple>`) when installed from npm, and falls back to a
// locally built `spatial_rules.node` during development.

import { createRequire } from 'node:module';
import { dirname, join } from 'node:path';
import { fileURLToPath } from 'node:url';

const require = createRequire(import.meta.url);
const here = dirname(fileURLToPath(import.meta.url));

// Map this process to a published per-platform package (ADR-0006: win32 +
// linux x64/arm64, gnu/musl).
function platformPackage() {
  const { platform, arch } = process;
  if (platform === 'win32') {
    return arch === 'arm64' ? 'spatial-rules-win32-arm64-msvc' : 'spatial-rules-win32-x64-msvc';
  }
  if (platform === 'linux') {
    const cpu = arch === 'arm64' ? 'arm64' : 'x64';
    let isMusl = false;
    try {
      const report = process.report?.getReport?.();
      isMusl = !report?.header?.glibcVersionRuntime;
    } catch {
      isMusl = false;
    }
    return `spatial-rules-linux-${cpu}-${isMusl ? 'musl' : 'gnu'}`;
  }
  return null;
}

let native = null;
const pkg = platformPackage();
if (pkg) {
  try {
    native = require(pkg);
  } catch {
    native = null;
  }
}
if (!native) {
  native = require(join(here, 'spatial_rules.node'));
}

export class SpatialRulesError extends Error {
  constructor(message, code) {
    super(message);
    this.name = 'SpatialRulesError';
    this.code = code;
  }
}

function rethrow(err) {
  if (err && typeof err.code === 'string' && err.code.startsWith('SR_')) {
    throw new SpatialRulesError(err.message, err.code);
  }
  // Async rejections cannot carry a custom `SR_*` in `.code` (napi forces a
  // Status enum code), so the addon embeds the code in the message instead.
  if (err && typeof err.message === 'string') {
    const match = /^(SR_[A-Z0-9_]+): ([\s\S]*)$/.exec(err.message);
    if (match) {
      throw new SpatialRulesError(match[2], match[1]);
    }
  }
  throw err;
}

export class SpatialRuleset {
  constructor(rules) {
    try {
      this._native = new native.SpatialRuleset(rules);
    } catch (err) {
      rethrow(err);
    }
  }

  query(candidates, query) {
    try {
      return this._native.query(candidates, query);
    } catch (err) {
      rethrow(err);
    }
  }

  async queryAsync(candidates, query) {
    try {
      return await this._native.queryAsync(candidates, query);
    } catch (err) {
      rethrow(err);
    }
  }

  queryRich(candidates, query) {
    try {
      return this._native.queryRich(candidates, query);
    } catch (err) {
      rethrow(err);
    }
  }

  replace(rules) {
    try {
      return this._native.replace(rules);
    } catch (err) {
      rethrow(err);
    }
  }

  toJSON() {
    try {
      return this._native.toJSON();
    } catch (err) {
      rethrow(err);
    }
  }

  fromCanonical(rules) {
    try {
      return this._native.fromCanonical(rules);
    } catch (err) {
      rethrow(err);
    }
  }

  stats() {
    try {
      return this._native.stats();
    } catch (err) {
      rethrow(err);
    }
  }
}
