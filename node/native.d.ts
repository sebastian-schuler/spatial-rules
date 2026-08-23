// Hand-written types for the napi addon surface (ADR-0006, Q17=b — not
// `napi build` typegen). Kept in sync with `node/src/lib.rs`; if a signature
// drifts from the Rust `#[napi]` surface, the typecheck won't catch it — the
// wrapper tests are the real gate.
//
// The wrapper loads the addon dynamically (`createRequire` of a per-platform
// package or a local `spatial_rules.node`), so this file is type-only input:
// it is never emitted, imported, or shipped. Consumers get the wrapper's own
// `dist/index.d.ts` types.

/**
 * The native `SpatialRuleset` class exposed by the addon. All JSON-returning
 * methods return strings (serialized in Rust).
 */
export declare class SpatialRuleset {
    constructor(rules: Buffer);
    /** Mask aligned to input candidates: `0` no match, `1` matched, `2` invalid. */
    query(candidates: Buffer, query: string): Uint8Array;
    /** Same mask as `query`, computed off the main thread. */
    queryAsync(candidates: Buffer, query: string): Promise<Uint8Array>;
    /** Per-candidate rich outcomes (string rule ids, optional overlaps) as JSON. */
    queryRich(candidates: Buffer, query: string): string;
    /** Resolution mask aligned to input candidates: `0` no resolution, `1` resolved, `2` invalid. */
    resolve(candidates: Buffer, query: string): Uint8Array;
    /** Same resolution mask as `resolve`, computed off the main thread. */
    resolveAsync(candidates: Buffer, query: string): Promise<Uint8Array>;
    /** Per-candidate resolution outcomes (winner, values, applicable) as JSON. */
    resolveRich(candidates: Buffer, query: string): string;
    /** Atomically swap the ruleset; ADR-0007 observability report as JSON. */
    replace(rules: Buffer): string;
    /** Canonical (validated) ruleset serialization (ADR-0013). */
    toJSON(): string;
    /** Atomic replace from canonical JSON; observability report as JSON. */
    fromCanonical(rules: Buffer): string;
    /** Observability for the current ruleset as JSON. */
    stats(): string;
}
