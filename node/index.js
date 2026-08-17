// Thin JS wrapper over the native addon (ADR-0005, ADR-0006).
//
// Defines `SpatialRulesError extends Error` with a `.code` property, and
// re-throws native errors (which carry `SR_*` codes) as that class.

import { createRequire } from 'node:module';
import { dirname, join } from 'node:path';
import { fileURLToPath } from 'node:url';

const require = createRequire(import.meta.url);
const native = require(join(dirname(fileURLToPath(import.meta.url)), 'spatial_rules.node'));

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

  queryRich(candidates, query) {
    try {
      return this._native.queryRich(candidates, query);
    } catch (err) {
      rethrow(err);
    }
  }
}
