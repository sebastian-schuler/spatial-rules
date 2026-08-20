# Migrate the wrapper `index.js` → `index.ts` (compile-on-publish)

Status: resolved
Blocked by: 01
Type: task

## Scope

- Port `node/index.js` → `node/index.ts` with full types on `SpatialRulesError`, `QueryResult`, and `SpatialRuleset`, importing the native addon through `native.d.ts` (ticket 01).
- **Erasable-only TS** (spec constraint): no enums, no namespaces, no parameter properties.
- Runtime semantics identical: input normalization (`toGeoJsonBuffer`/`toQueryString`), `rethrow`, the `QueryResult` chain (mask/indices/summary/count/toGeoJson/toRichJson), lazy rich call.
- Publish wiring (Q18=a): `main` → `dist/index.js`; `types` → `dist/index.d.ts` (or root `index.d.ts`); `files` → include `dist/` (compiled JS + types) + `npm/`; `prepublishOnly` runs the `tsc` emit (with `noEmit` off) before pack. `npm run build`/`build:debug` (napi) stays as-is — the wrapper emit is separate from the addon build.

## Notes

- Type the `native` import loosely if needed (typed addon surface is intentionally small); the wrapper's own exported types are the deliverable.
- Keep `Buffer`/`Uint8Array`/`Uint32Array` types from `node:` — no DOM lib assumptions in the tsconfig.

## Done criteria

- `tsc --noEmit` clean in `node/`.
- `node test/smoke.mjs` still passes against the unchanged `.mjs` test (regression check before ticket 03) — i.e. the TS wrapper is loadable by the old test through Node type stripping or a temporary shim.
- `bun` loads and runs the wrapper (`bun -e "import('./index.ts')..."` smoke).
- Packed tarball installs and resolves `main` → compiled `dist/index.js` (manual `npm pack` + install check).

## Answer

- `node/index.ts`: full types on `SpatialRulesError` (`.code: string`), `QueryResult` (mask/indices/summary/count/toGeoJson/toRichJson + `QuerySummary` interface), `SpatialRuleset`; exported input types `GeoJsonInput` / `QueryInput`. Erasable-only (no enums/namespaces/parameter properties) — verified by `erasableSyntaxOnly` + Node type stripping.
- `native.d.ts` is consumed via `import type`; `NativeModule` is derived from that file's namespace (`import type * as` + `typeof`), and `QueryResult` depends on a local structural `RichQuerySource` interface, so the emitted `dist/index.d.ts` never references the addon path (self-contained, verified).
- Native crossings go through one `callNative` guard (plus an async `callNativeAsync` so rejections are awaited inside the guard) instead of a try/catch per method; `rethrow` semantics identical. Loader restructured into `loadNative()` returning a `const` (TS cannot narrow a mutable module-level `let` inside methods); runtime behavior identical.
- Publish wiring: `main`/`types` -> `dist/index.*`, `files: [dist/, npm/]`, `build:ts` = `tsc -p tsconfig.build.json` (emit config separate because `allowImportingTsExtensions` forbids emit), `prepack` = tsc emit (runs on both `npm pack` and `npm publish`; `prepublishOnly` alone would ship pack tarballs without `dist/`), `prepublishOnly` = `napi prepublish`. `node/dist/` gitignored.
- Collateral (necessary, out-of-scope files kept `.mjs`): `integration/server.mjs` + `memory.mjs` import `../node/index.ts` (run under Bun), Dockerfile `COPY node/index.ts`, and `test/smoke.mjs`'s import switched to `../index.ts` as the ticket's temporary shim.
- Verified: `tsc --noEmit` clean; `node test/smoke.mjs` green (type stripping via the shim); `bun -e "import('./index.ts')"` loads + constructs; `npm pack` ships `dist/index.js` + `dist/index.d.ts` and a fresh install resolves `main` and passes `clean-install.mjs`.

