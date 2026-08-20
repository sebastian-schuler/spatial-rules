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
import type { SpatialRuleset as NativeRuleset } from './native.js';

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

/** The addon module shape as loaded by the require() calls below. */
type NativeModule = { SpatialRuleset: typeof NativeRuleset };

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

export class SpatialRulesError extends Error {
  code: string;

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

// A chainable evaluation result (filtering-scale ticket 03): one native query
// call computes the mask; every terminal derives from it without another
// crossing, except `toRichJson()`, which lazily makes one native rich call on
// first use (ADR-0012). Memory: the mask is the minimal primitive (1 byte per
// candidate); the heavy views (`toGeoJson`/`toRichJson`) are one-shot and
// never cached, so results stay lean for giant lists.
export class QueryResult {
  private _native: RichQuerySource;
  private _candidates: Buffer;
  private _query: string;
  private _mask: Uint8Array;
  private _rich: string | null;

  constructor(native: RichQuerySource, candidates: Buffer, query: string, mask: Uint8Array) {
    this._native = native;
    this._candidates = candidates;
    this._query = query;
    this._mask = mask;
    this._rich = null;
  }

  // The primitive: a Uint8Array mask aligned to the input candidates
  // (0 = no match, 1 = matched, 2 = invalid).
  toMask(): Uint8Array {
    return this._mask;
  }

  // Indices of matched candidates (mask === 1), ascending, aligned to input.
  // Exactly sized — no oversized transient buffer (memory-lean for giant
  // lists).
  toIndices(): Uint32Array {
    return this._indicesWhere((value) => value === 1);
  }

  // Indices of invalid candidates (mask === 2), ascending — the positions a
  // caller should skip rather than treat as filtered out.
  invalidIndices(): Uint32Array {
    return this._indicesWhere((value) => value === 2);
  }

  // A count breakdown of the batch: matched / notMatched / invalid. Cheap —
  // prefer this before materialising a heavy view (toGeoJson/toRichJson).
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

  // Number of matched candidates.
  count(): number {
    let n = 0;
    for (let i = 0; i < this._mask.length; i += 1) if (this._mask[i] === 1) n += 1;
    return n;
  }

  // The matched candidates as a GeoJSON FeatureCollection string, in input
  // order, with every original property preserved (kept from the original
  // payload — no lossy round-trip through the engine's parsed candidates).
  // Unmatched and invalid candidates are dropped.
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

  // Per-candidate rich outcomes (original string rule ids, optional overlap
  // metrics) as a JSON string. Lazy: one native call on first use, then
  // cached (ADR-0012). Unlike the mask (captured at query time), this is
  // evaluated against the ruleset current at first call — a replace() between
  // `query()` and `toRichJson()` can make the two disagree; the mask wins.
  toRichJson(): string {
    if (this._rich === null) {
      try {
        this._rich = this._native.queryRich(this._candidates, this._query);
      } catch (err) {
        rethrow(err);
      }
    }
    return this._rich;
  }
}

export class SpatialRuleset {
  private _native: NativeRuleset;

  constructor(rules: GeoJsonInput) {
    const normalized = toGeoJsonBuffer(rules, 'rules');
    try {
      this._native = new native.SpatialRuleset(normalized);
    } catch (err) {
      rethrow(err);
    }
  }

  query(candidates: GeoJsonInput, query: QueryInput): QueryResult {
    const normalizedCandidates = toGeoJsonBuffer(candidates, 'candidates');
    const normalizedQuery = toQueryString(query);
    try {
      const mask = this._native.query(normalizedCandidates, normalizedQuery);
      return new QueryResult(this._native, normalizedCandidates, normalizedQuery, mask);
    } catch (err) {
      rethrow(err);
    }
  }

  async queryAsync(candidates: Buffer, query: string): Promise<Uint8Array> {
    try {
      return await this._native.queryAsync(candidates, query);
    } catch (err) {
      rethrow(err);
    }
  }

  queryRich(candidates: Buffer, query: string): string {
    try {
      return this._native.queryRich(candidates, query);
    } catch (err) {
      rethrow(err);
    }
  }

  replace(rules: GeoJsonInput): string {
    const normalized = toGeoJsonBuffer(rules, 'rules');
    try {
      return this._native.replace(normalized);
    } catch (err) {
      rethrow(err);
    }
  }

  toJSON(): string {
    try {
      return this._native.toJSON();
    } catch (err) {
      rethrow(err);
    }
  }

  fromCanonical(rules: Buffer): string {
    try {
      return this._native.fromCanonical(rules);
    } catch (err) {
      rethrow(err);
    }
  }

  stats(): string {
    try {
      return this._native.stats();
    } catch (err) {
      rethrow(err);
    }
  }
}
