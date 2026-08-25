// Thin TS wrapper over the wasm-bindgen build of the core (`.scratch/wasm`,
// ticket 01) — the Ruleset-level surface, mirroring `node/index.ts` minus the
// Engine-level `replace`/`stats` and the async paths.
//
// The generated `pkg/spatial_rules_wasm.js` imports the `.wasm` as an ES
// module, so `SpatialRuleset` is usable synchronously in any runtime that
// supports wasm ESM (bundlers, Deno, Node with `--experimental-wasm-modules`);
// there is no async bootstrap.
//
// Erasable-only TS: these sources run under both Node (type stripping) and
// Deno, and are compiled to `dist/` for publish.

import { SpatialRuleset as NativeRuleset } from './pkg/spatial_rules_wasm.js';

/** GeoJSON input accepted for rules and candidates: GeoJSON string | bytes | object. */
export type GeoJsonInput = string | Uint8Array | Record<string, unknown>;

/** Query input: JSON string or object. */
export type QueryInput = string | Record<string, unknown>;

/** Count breakdown of a batch evaluation. */
export interface QuerySummary {
  matched: number;
  notMatched: number;
  invalid: number;
}

/** Count breakdown of a batch resolution (ADR-0015). */
export interface ResolutionSummary {
  resolved: number;
  notResolved: number;
  invalid: number;
}

/**
 * Error thrown by the wrapper for any native failure carrying an `SR_*` code
 * (ADR-0005). The wasm methods throw a JS `Error` whose message is
 * `"SR_CODE: message"`; this reconstructs the coded error the napi path
 * throws.
 */
export class SpatialRulesError extends Error {
  /** Machine-readable `SR_*` error code identifying the failure. */
  code: string;

  constructor(message: string, code: string) {
    super(message);
    this.name = 'SpatialRulesError';
    this.code = code;
  }
}

function rethrow(err: unknown): never {
  const maybe = err as { message?: unknown } | null | undefined;
  if (maybe && typeof maybe.message === 'string') {
    const match = /^(SR_[A-Z0-9_]+): ([\s\S]*)$/.exec(maybe.message);
    if (match) {
      throw new SpatialRulesError(match[2], match[1]);
    }
  }
  throw err;
}

// Run one native crossing, re-throwing any error through `rethrow` as a
// `SpatialRulesError` — a single guard instead of a try/catch per call site.
function callNative<T>(fn: () => T): T {
  try {
    return fn();
  } catch (err) {
    rethrow(err);
  }
}

// Input normalization, reimplemented in-package (the browser package must not
// import the Node addon's module). Rules/candidates and the query accept
// string | object (Buffer is a Uint8Array, decoded via TextDecoder).
function toGeoJsonString(value: GeoJsonInput, what: string): string {
  if (typeof value === 'string') return value;
  if (value instanceof Uint8Array) return new TextDecoder().decode(value);
  if (value !== null && typeof value === 'object' && !Array.isArray(value)) {
    return JSON.stringify(value);
  }
  throw new TypeError(`${what} must be a GeoJSON string, a Uint8Array, or a GeoJSON object`);
}

function toQueryString(value: QueryInput): string {
  if (typeof value === 'string') return value;
  if (value !== null && typeof value === 'object' && !Array.isArray(value)) {
    return JSON.stringify(value);
  }
  throw new TypeError('query must be a JSON string or an object');
}

// One pass over the mask yielding the 1/2/0 counts, shared by the match and
// resolution result classes (masked results differ only in how they label the
// three buckets).
function maskCounts(mask: Uint8Array): { one: number; zero: number; two: number } {
  let one = 0;
  let zero = 0;
  let two = 0;
  for (let i = 0; i < mask.length; i += 1) {
    if (mask[i] === 1) one += 1;
    else if (mask[i] === 2) two += 1;
    else zero += 1;
  }
  return { one, zero, two };
}

/**
 * A chainable evaluation result from one wasm query call, mirroring the Node
 * wrapper's `QueryResult`. Every terminal view derives from a single computed
 * mask; `toOutcomesJson()` makes the one wasm rich call on first use.
 */
export class QueryResult {
  private native: NativeRuleset;
  private candidates: string;
  private query: string;
  private _mask: Uint8Array;
  private rich: string | null;

  constructor(native: NativeRuleset, candidates: string, query: string, mask: Uint8Array) {
    this.native = native;
    this.candidates = candidates;
    this.query = query;
    this._mask = mask;
    this.rich = null;
  }

  /** The primitive result: a `Uint8Array` mask aligned to the candidates. */
  mask(): Uint8Array {
    return this._mask;
  }

  /** Sorted indices of the matched candidates (mask `=== 1`). */
  indices(): Uint32Array {
    return this._indicesWhere((value) => value === 1);
  }

  /** Sorted indices of the invalid candidates (mask `=== 2`). */
  invalidIndices(): Uint32Array {
    return this._indicesWhere((value) => value === 2);
  }

  /** The matched / notMatched / invalid counts. */
  summary(): QuerySummary {
    const { one, zero, two } = maskCounts(this._mask);
    return { matched: one, notMatched: zero, invalid: two };
  }

  /** Number of matched candidates. */
  count(): number {
    return maskCounts(this._mask).one;
  }

  // Exact-size index array for the positions whose mask value passes `test`.
  private _indicesWhere(test: (value: number) => boolean): Uint32Array {
    let n = 0;
    for (let i = 0; i < this._mask.length; i += 1) if (test(this._mask[i])) n += 1;
    const indices = new Uint32Array(n);
    let k = 0;
    for (let i = 0; i < this._mask.length; i += 1) if (test(this._mask[i])) indices[k++] = i;
    return indices;
  }

  /** The matched candidates as a GeoJSON FeatureCollection string, input order. */
  toGeoJson(): string {
    const parsed = JSON.parse(this.candidates);
    const features = parsed.type === 'FeatureCollection' ? parsed.features : [parsed];
    const kept = [];
    for (let i = 0; i < this._mask.length; i += 1) {
      if (this._mask[i] === 1) kept.push(features[i]);
    }
    return JSON.stringify({ type: 'FeatureCollection', features: kept });
  }

  /**
   * Per-candidate outcomes (string rule ids, optional overlap/aggregate
   * payloads) as a JSON string. One wasm rich call on first use, then cached.
   */
  toOutcomesJson(): string {
    if (this.rich === null) {
      this.rich = callNative(() => this.native.query_rich(this.candidates, this.query));
    }
    return this.rich;
  }
}

/**
 * A chainable resolution result from one wasm resolve call, mirroring the Node
 * wrapper's `ResolutionResult` (ADR-0015).
 */
export class ResolutionResult {
  private native: NativeRuleset;
  private candidates: string;
  private query: string;
  private _mask: Uint8Array;
  private json: string | null;

  constructor(native: NativeRuleset, candidates: string, query: string, mask: Uint8Array) {
    this.native = native;
    this.candidates = candidates;
    this.query = query;
    this._mask = mask;
    this.json = null;
  }

  /** The primitive result: a `Uint8Array` mask (`0` no resolution, `1` resolved, `2` invalid). */
  mask(): Uint8Array {
    return this._mask;
  }

  /** Number of resolved candidates. */
  count(): number {
    return maskCounts(this._mask).one;
  }

  /** The resolved / notResolved / invalid counts. */
  summary(): ResolutionSummary {
    const { one, zero, two } = maskCounts(this._mask);
    return { resolved: one, notResolved: zero, invalid: two };
  }

  /**
   * Per-candidate resolution outcomes as a JSON string: `{outcome, winner,
   * values, applicable}` (+ `aggregate`) for resolved candidates (ADR-0015).
   * One wasm rich call on first use, then cached.
   */
  toJson(): string {
    if (this.json === null) {
      this.json = callNative(() => this.native.resolve_rich(this.candidates, this.query));
    }
    return this.json;
  }
}

/**
 * A compiled spatial ruleset (the wasm Ruleset-level surface). Build once,
 * then evaluate candidate batches. Same input normalization and result shapes
 * as the Node wrapper; no `replace`/`stats` (Engine observability is out of
 * scope on wasm) and no async paths.
 */
export class SpatialRuleset {
  private native: NativeRuleset;

  constructor(rules: GeoJsonInput) {
    this.native = callNative(() => new NativeRuleset(toGeoJsonString(rules, 'rules')));
  }

  /**
   * Evaluate `query` against `candidates` and return a chainable `QueryResult`.
   */
  query(candidates: GeoJsonInput, query: QueryInput): QueryResult {
    const candidatesString = toGeoJsonString(candidates, 'candidates');
    const queryString = toQueryString(query);
    const mask = callNative(() => this.native.query(candidatesString, queryString));
    return new QueryResult(this.native, candidatesString, queryString, mask);
  }

  /**
   * Resolve `query` against `candidates` and return a chainable
   * `ResolutionResult` (ADR-0015).
   */
  resolve(candidates: GeoJsonInput, query: QueryInput): ResolutionResult {
    const candidatesString = toGeoJsonString(candidates, 'candidates');
    const queryString = toQueryString(query);
    const mask = callNative(() => this.native.resolve(candidatesString, queryString));
    return new ResolutionResult(this.native, candidatesString, queryString, mask);
  }

  /** The validated rules as canonical JSON (ADR-0013). */
  toCanonical(): string {
    return callNative(() => this.native.to_canonical());
  }
}