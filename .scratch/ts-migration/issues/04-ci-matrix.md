# CI matrix: typecheck step + TS tests under Node and Bun

Status: ready-for-agent
Blocked by: 03
Type: task

## Scope

Wire `.github/workflows/test.yml` for the migrated layer:

- **node job** (Node 22/24/26, ubuntu + windows):
  - Add a `tsc --noEmit` typecheck step (after the addon build).
  - Run `node test/smoke.ts` with the correct type-stripping invocation **per Node version**: verify whether each of 22/24/26 strips types by default or needs `--experimental-strip-types`; pin the command so the matrix is green (e.g. `node --experimental-strip-types test/smoke.ts` if the flag is accepted across all three, or a version-conditional command).
- **bun job**: `bun test/smoke.ts` (native TS) — the addon copy step unchanged.
- **clean-install job**: unchanged flow, now exercising the compiled `dist/index.js` artifact; `node clean-install.ts` under plain Node.

## Notes

- If Node 22 in CI cannot run TS sources (old flag semantics), the fallback is to run the node-job smoke under Bun only for that version, or bump the matrix floor — surface this in the ticket answer rather than silently dropping Node 22.
- Local benchmark harness (`bench.mjs`, `benchmarks/js/*`) stays `.mjs` under Bun — do not touch.

## Done criteria

- All three jobs (`rust`, `node`, `bun`, `clean-install`) green on the migrated files.
- Typecheck step runs and is clean in CI.
- The Node matrix (22/24/26) runs the TS smoke test on both ubuntu and windows.