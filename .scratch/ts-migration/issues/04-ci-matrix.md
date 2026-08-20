# CI matrix: typecheck step + TS tests under Node and Bun

Status: resolved
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

## Answer

- **node job**: added `npm install` + `Typecheck (tsc --noEmit)` steps after the addon build; smoke is `node --experimental-strip-types test/smoke.ts` — the flag is accepted across the whole matrix (introduced 22.6, still valid where stripping is default-on in 24/26; verified locally on Node 24), so one command covers 22/24/26 on ubuntu + windows.
- Flag verification closed post-review: verified locally on **22.18.0, 24.18.0, and 26.7.0** (nvm) — `--experimental-strip-types` is accepted on all three, and stripping is in fact default-on on each (bare `node test/smoke.ts` also passes), so the single pinned command is green across the whole matrix.
- **bun job**: `bun test/smoke.ts`, addon copy step unchanged.
- **clean-install job**: added `npm install` in `node/` before packing — `npm pack` runs `prepack` (not `prepublishOnly`), so the tsc emit must find `typescript`. Found + fixed a real trap here: the emit was first wired into `prepublishOnly`, which `npm pack` never runs — the CI tarball would have shipped without `dist/`. Moved the emit to `prepack` (runs on both pack and publish); verified locally that a pack from a clean tree builds `dist/` and includes it in the tarball. The install loop now sets `type=module` on the temp project (`.ts` is not implicitly ESM like `.mjs`) and runs `node --experimental-strip-types clean-install.ts` (job pins Node 22, where the flag is required). Full job flow simulated locally — green.
- Docs collateral: `README.md` dev commands and `docs/test-matrix.md` owner row updated to `smoke.ts`.