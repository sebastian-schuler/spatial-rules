// Thin TS wrapper over the native addon (ADR-0005, ADR-0006).
//
// Defines `SpatialRulesError extends Error` with a `.code` property, and
// re-throws native errors (which carry `SR_*` codes) as that class.
//
// The native binary resolves from the per-platform optionalDependency package
// (`spatial-rules-<triple>`) when installed from npm, and falls back to a
// locally built `spatial_rules.node` during development.
//
// Erasable-only TS (spec constraint): these sources run under both Node
// (type stripping) and Bun (native TS), and are compiled to `dist/` for
// publish.

import { createRequire } from 'node:module';
import { dirname, join } from 'node:path';
import { fileURLToPath } from 'node:url';
// The addon module shape (ticket 01: declared by `native.d.ts` itself) —
// derived from that file's namespace, so the shape lives in one place only.
import type * as nativeModule from './native.js';
import type { SpatialRuleset as NativeRuleset } from './native.js';

type NativeModule = typeof nativeModule;

const require = createRequire(import.meta.url);
const here = dirname(fileURLToPath(import.meta.url));

/** GeoJSON input accepted for rules and candidates: Buffer | string | object. */
export type GeoJsonInput = Buffer | string | Record<string, unknown>;

/** Query input: JSON string or object. */
export type QueryInput = string | Record<string, unknown>;

/** Count breakdown of a batch evaluation. */
export interface QuerySummary {
  matched: number;
  notMatched: number;
  invalid: number;
}

/**
 * Structural slice of the native ruleset that `QueryResult` needs. Local (not
 * from `native.d.ts`) so the emitted `dist/index.d.ts` never references the
 * addon's module path.
 */
interface RichQuerySource {
  queryRich(candidates: Buffer, query: string): string;
}

// Map this process to a published per-platform package (ADR-0006: win32 +
// linux x64/arm64, gnu/musl).
function platformPackage(): string | null {
  const { platform, arch } = process;
  if (platform === 'win32') {
    return arch === 'arm64' ? 'spatial-rules-win32-arm64-msvc' : 'spatial-rules-win32-x64-msvc';
  }
  if (platform === 'linux') {
    const cpu = arch === 'arm64' ? 'arm64' : 'x64';
    let isMusl = false;
    try {
      const report = process.report?.getReport?.() as
        | { header?: { glibcVersionRuntime?: string } }
        | undefined;
      isMusl = !report?.header?.glibcVersionRuntime;
    } catch {
      isMusl = false;
    }
    return `spatial-rules-linux-${cpu}-${isMusl ? 'musl' : 'gnu'}`;
  }
  return null;
}

function loadNative(): NativeModule {
  const pkg = platformPackage();
  if (pkg) {
    try {
      return require(pkg) as NativeModule;
    } catch {
      // Platform package present in `optionalDependencies` but not installed
      // (or failed to load) — fall back to a local build below.
    }
  }
  return require(join(here, 'spatial_rules.node')) as NativeModule;
}

const native = loadNative();

/**
 * Error thrown by the wrapper for any native failure carrying an `SR_*`
 * code. Extends the built-in `Error` and adds a `code` property so callers
 * can branch on the specific failure without parsing messages.
 */
export class SpatialRulesError extends Error {
  /** Machine-readable `SR_*` error code identifying the failure. */
  code: string;

  /**
   * @param message - Human-readable description of the failure.
   * @param code - Machine-readable `SR_*` error code.
   */
  constructor(message: string, code: string) {
    super(message);
    this.name = 'SpatialRulesError';
    this.code = code;
  }
}

function rethrow(err: unknown): never {
  const maybe = err as { code?: unknown; message?: unknown } | null | undefined;
  if (maybe && typeof maybe.code === 'string' && maybe.code.startsWith('SR_')) {
    throw new SpatialRulesError(maybe.message as string, maybe.code);
  }
  // Async rejections cannot carry a custom `SR_*` in `.code` (napi forces a
  // Status enum code), so the addon embeds the code in the message instead.
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

// Async variant: the rejection must be awaited *inside* the guard, or it
// escapes as a plain rejected promise without the SpatialRulesError mapping.
async function callNativeAsync<T>(fn: () => Promise<T>): Promise<T> {
  try {
    return await fn();
  } catch (err) {
    rethrow(err);
  }
}

// Input normalization (filtering-scale ticket 05): the native boundary stays
// Buffer-in/mask-out (ADR-0006); the wrapper accepts Buffer | GeoJSON string |
// GeoJSON object for candidates and rules, and string | object for the query.
// A Buffer fast-path passes through untouched (byte-faithful); anything else
// throws a clear TypeError before the native crossing.
function toGeoJsonBuffer(value: GeoJsonInput, what: string): Buffer {
  if (Buffer.isBuffer(value)) return value;
  if (typeof value === 'string') return Buffer.from(value);
  if (value !== null && typeof value === 'object' && !Array.isArray(value)) {
    return Buffer.from(JSON.stringify(value));
  }
  throw new TypeError(`${what} must be a Buffer, a GeoJSON string, or a GeoJSON object`);
}

function toQueryString(value: QueryInput): string {
  if (typeof value === 'string') return value;
  if (value !== null && typeof value === 'object' && !Array.isArray(value)) {
    return JSON.stringify(value);
  }
  throw new TypeError('query must be a JSON string or an object');
}

/**
 * A chainable evaluation result from one native query call. Every terminal
 * view is derived from a single computed mask without another native
 * crossing, except `toRichJson()`, which lazily makes one native rich call on
 * first use (ADR-0012). Memory: the mask is the minimal primitive (1 byte per
 * candidate); the heavy views (`toGeoJson`/`toRichJson`) are one-shot and
 * never cached, so results stay lean for giant lists.
 */
export class QueryResult {
  private _native: RichQuerySource;
  private _candidates: Buffer;
  private _query: string;
  private _mask: Uint8Array;
  private _rich: string | null;

  /**
   * Construct a result from a native query call. You generally won't call
   * this directly — obtain one via `SpatialRuleset.query()`.
   */
  constructor(native: RichQuerySource, candidates: Buffer, query: string, mask: Uint8Array) {
    this._native = native;
    this._candidates = candidates;
    this._query = query;
    this._mask = mask;
    this._rich = null;
  }

  /**
   * The primitive result: a `Uint8Array` mask aligned to the input
   * candidates (`0` = no match, `1` = matched, `2` = invalid).
   * @returns The raw mask, one byte per candidate.
   */
  toMask(): Uint8Array {
    return this._mask;
  }

  /**
   * Indices of matched candidates (mask `=== 1`), ascending, aligned to
   * input. Exactly sized — no oversized transient buffer (memory-lean for
   * giant lists).
   * @returns Sorted indices of the matched candidates.
   */
  toIndices(): Uint32Array {
    return this._indicesWhere((value) => value === 1);
  }

  /**
   * Indices of invalid candidates (mask `=== 2`), ascending — the positions
   * a caller should skip rather than treat as filtered out.
   * @returns Sorted indices of the invalid candidates.
   */
  invalidIndices(): Uint32Array {
    return this._indicesWhere((value) => value === 2);
  }

  /**
   * A count breakdown of the batch: matched / notMatched / invalid. Cheap —
   * prefer this before materialising a heavy view (`toGeoJson`/`toRichJson`).
   * @returns The matched, notMatched, and invalid counts.
   */
  summary(): QuerySummary {
    let matched = 0;
    let notMatched = 0;
    let invalid = 0;
    for (let i = 0; i < this._mask.length; i += 1) {
      if (this._mask[i] === 1) matched += 1;
      else if (this._mask[i] === 2) invalid += 1;
      else notMatched += 1;
    }
    return { matched, notMatched, invalid };
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

  /**
   * Number of matched candidates.
   * @returns The count of candidates whose mask value is `1`.
   */
  count(): number {
    let n = 0;
    for (let i = 0; i < this._mask.length; i += 1) if (this._mask[i] === 1) n += 1;
    return n;
  }

  /**
   * The matched candidates as a GeoJSON FeatureCollection string, in input
   * order, with every original property preserved (kept from the original
   * payload — no lossy round-trip through the engine's parsed candidates).
   * Unmatched and invalid candidates are dropped.
   * @returns A GeoJSON `FeatureCollection` string of the matched features.
   */
  toGeoJson(): string {
    const raw = Buffer.isBuffer(this._candidates)
      ? this._candidates.toString('utf8')
      : String(this._candidates);
    const parsed = JSON.parse(raw);
    const features = parsed.type === 'FeatureCollection' ? parsed.features : [parsed];
    const kept = [];
    for (let i = 0; i < this._mask.length; i += 1) {
      if (this._mask[i] === 1) kept.push(features[i]);
    }
    return JSON.stringify({ type: 'FeatureCollection', features: kept });
  }

  /**
   * Per-candidate rich outcomes (original string rule ids, optional overlap
   * metrics) as a JSON string. Lazy: one native call on first use, then
   * cached (ADR-0012). Unlike the mask (captured at query time), this is
   * evaluated against the ruleset current at first call — a `replace()`
   * between `query()` and `toRichJson()` can make the two disagree; the mask
   * wins.
   * @returns A JSON string of per-candidate rich outcomes.
   */
  toRichJson(): string {
    if (this._rich === null) {
      this._rich = callNative(() => this._native.queryRich(this._candidates, this._query));
    }
    return this._rich;
  }
}

/**
 * A compiled spatial ruleset. Build one from GeoJSON rules, then query
 * candidate features against it. All JSON-returning methods return strings
 * (serialized in Rust). Every method re-throws native failures as
 * `SpatialRulesError` with an `SR_*` code.
 */
export class SpatialRuleset {
  private _native: NativeRuleset;

  /**
   * @param rules - The rules to compile, as a Buffer, GeoJSON string, or
   *   GeoJSON object (e.g. a FeatureCollection of rule features).
   */
  constructor(rules: GeoJsonInput) {
    const normalized = toGeoJsonBuffer(rules, 'rules');
    this._native = callNative(() => new native.SpatialRuleset(normalized));
  }

  /**
   * Evaluate `query` against `candidates` and return a chainable `QueryResult`.
   * @param candidates - Candidate features: Buffer, GeoJSON string, or object.
   * @param query - The query to run: JSON string or object.
   * @returns A `QueryResult` whose terminals expose the match mask, indices,
   *   counts, GeoJSON, or rich outcomes.
   */
  query(candidates: GeoJsonInput, query: QueryInput): QueryResult {
    const normalizedCandidates = toGeoJsonBuffer(candidates, 'candidates');
    const normalizedQuery = toQueryString(query);
    const mask = callNative(() => this._native.query(normalizedCandidates, normalizedQuery));
    return new QueryResult(this._native, normalizedCandidates, normalizedQuery, mask);
  }

  /**
   * Same mask as `query`, computed off the main thread.
   * @param candidates - Candidate features as a Buffer.
   * @param query - The query as a JSON string.
   * @returns A promise of the mask (`0` no match, `1` matched, `2` invalid).
   */
  async queryAsync(candidates: Buffer, query: string): Promise<Uint8Array> {
    return callNativeAsync(() => this._native.queryAsync(candidates, query));
  }

  /**
   * Per-candidate rich outcomes (original string rule ids, optional overlap
   * metrics) as a JSON string.
   * @param candidates - Candidate features as a Buffer.
   * @param query - The query as a JSON string.
   * @returns A JSON string of per-candidate rich outcomes.
   */
  queryRich(candidates: Buffer, query: string): string {
    return callNative(() => this._native.queryRich(candidates, query));
  }

  /**
   * Atomically swap the ruleset. Returns the ADR-0007 observability report
   * as JSON.
   * @param rules - The replacement rules: Buffer, GeoJSON string, or object.
   * @returns The observability report as a JSON string.
   */
  replace(rules: GeoJsonInput): string {
    const normalized = toGeoJsonBuffer(rules, 'rules');
    return callNative(() => this._native.replace(normalized));
  }

  /**
   * Canonical (validated) ruleset serialization (ADR-0013).
   * @returns The canonical ruleset as a JSON string.
   */
  toJSON(): string {
    return callNative(() => this._native.toJSON());
  }

  /**
   * Atomic replace from canonical JSON; returns the observability report as
   * JSON.
   * @param rules - Canonical ruleset serialization as a Buffer.
   * @returns The observability report as a JSON string.
   */
  fromCanonical(rules: Buffer): string {
    return callNative(() => this._native.fromCanonical(rules));
  }

  /**
   * Observability for the current ruleset as JSON.
   * @returns The observability report as a JSON string.
   */
  stats(): string {
    return callNative(() => this._native.stats());
  }
}
