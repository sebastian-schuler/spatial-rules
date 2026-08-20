# Migrate the node tests `smoke.mjs` + `clean-install.mjs` → `.ts`

Status: resolved
Blocked by: 02
Type: task

## Scope

- `node/test/smoke.mjs` → `node/test/smoke.ts`: import `../index.ts` (the TS wrapper) instead of `../index.js`; keep every assertion; keep it runnable under **both** runtimes:
  - Node: type stripping (`node --experimental-strip-types test/smoke.ts` on Node 22.6+; default-on on 23.6+/24+ — see CI ticket 04 for the matrix pinning).
  - Bun: `bun test/smoke.ts` (native TS).
- `node/test/clean-install.mjs` → `node/test/clean-install.ts`: this exercises the **installed compiled package** (`dist/index.js`), so its types are thin; keep the install/require flow identical.
- Update `node/package.json` `test` script to the new invocation.

## Notes

- Node version caveat: type stripping is default-on from Node 23.6 and backported to later 22.x lines, but Node 22.6+ needs `--experimental-strip-types`. The CI ticket (04) verifies the exact matrix; this ticket should validate locally on the installed Node.

## Done criteria

- `node test/smoke.ts` (with the appropriate stripping flag) passes on the local Node.
- `bun test/smoke.ts` passes.
- `tsc --noEmit` covers both test files and is clean.
- `clean-install.ts` passes against a freshly packed + installed tarball (compiled `dist/index.js`).

## Answer

- `smoke.ts`: import changed to `../index.ts`; every assertion kept verbatim. The ten deliberately-invalid-input assertions go through a tiny `invalid(value: unknown) => never` helper so the compile-time signatures don't reject them while the runtime TypeError path is still exercised.
- `clean-install.ts`: flow identical; in-repo typecheck resolves `spatial-rules` through the tsconfig `paths` mapping to the TS wrapper source; at runtime in the temp project it resolves the installed `dist/index.js`.
- Caveat found: unlike `.mjs`, a copied `.ts` file is ESM only if the nearest package.json says `"type": "module"`. The clean-install loop must set that after `npm init -y` (wired in ticket 04; local check green after adding it).
- `test` script: `node --experimental-strip-types test/smoke.ts` — accepted and green on local Node 24 (default-on there) and required for Node 22.6+; `bun test/smoke.ts` green; `tsc --noEmit` covers both files.
- `bench.mjs` NODE_SMOKE path updated to `smoke.ts` (runs it under Bun, so no stripping concern).