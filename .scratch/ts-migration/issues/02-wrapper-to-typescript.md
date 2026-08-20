# Migrate the wrapper `index.js` → `index.ts` (compile-on-publish)

Status: ready-for-agent
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