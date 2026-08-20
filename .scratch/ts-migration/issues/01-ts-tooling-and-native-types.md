# Tooling bootstrap: tsconfig, typescript dep, `native.d.ts`

Status: resolved
Blocked by: (none)
Type: task

## Scope

Make the `node/` package TS-ready:

- Add `tsconfig.json` in `node/`:
  - `"module": "nodenext"`, `"moduleResolution": "nodenext"`, `"target": "es2022"`, `"strict": true`, `"erasableSyntaxOnly": true`.
  - Dev default `"noEmit": true`; publish emits to `dist/` via a `tsc` invocation (ticket 02 owns the wiring).
  - `"include": ["index.ts", "test/*.ts"]`.
- Add `typescript` to `node/package.json` `devDependencies`.
- Hand-write `node/native.d.ts` covering the native addon surface consumed by the wrapper:
  - `SpatialRuleset` class: `new (rules: Buffer)`, `query(candidates: Buffer, query: string) => Uint8Array`, `queryAsync(candidates: Buffer, query: string) => Promise<Uint8Array>`, `queryRich(candidates: Buffer, query: string) => string`, `replace(rules: Buffer) => string`, `toJSON() => string`, `fromCanonical(rules: Buffer) => string`, `stats() => string`.
  - Declare the module shape (default export / named `SpatialRuleset` as the wrapper imports it).
- Add a `typecheck` script (`tsc --noEmit`) to `node/package.json`.

## Notes

- The addon types are hand-written (Q17=b); keep them minimal and accurate to `node/src/lib.rs` (ADR-0006). If a method signature drifts from the Rust `#[napi]` surface, the typecheck won't catch it â€” the wrapper tests are the real gate.

## Done criteria

- `node` typecheck passes: `tsc --noEmit` in `node/` is clean.
- `cargo build` untouched; `npm` install resolves `typescript`.
- No wrapper/test file changed yet.
## Answer

- 
ode/tsconfig.json: nodenext/nodenext, es2022, strict, erasableSyntaxOnly, 
oEmit, llowImportingTsExtensions (tests import ../index.ts), 	ypes: ["node"], and a paths mapping spatial-rules -> ./index.ts so 	est/clean-install.ts typechecks in-repo against the wrapper source.
- 
ode/native.d.ts: hand-written addon surface (all 8 methods + constructor) mirroring 
ode/src/lib.rs. Type-only — never emitted or shipped.
- 
ode/package.json: 	ypescript ^5.9 + @types/node ^24 devDependencies; 	ypecheck script (	sc --noEmit).
- 	sc --noEmit clean; 
pm install resolves; no wrapper/test file touched. include adds 
ative.d.ts alongside the ticket's list so the program has inputs before index.ts exists (stage 01).

