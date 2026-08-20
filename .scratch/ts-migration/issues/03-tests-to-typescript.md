# Migrate the node tests `smoke.mjs` + `clean-install.mjs` → `.ts`

Status: ready-for-agent
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