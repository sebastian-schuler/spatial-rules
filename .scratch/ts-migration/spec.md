# TypeScript migration of the Node/Bun JS layer

Migrate the published JS wrapper and its tests from `.js`/`.mjs` to TypeScript, keeping both Node and Bun as supported runtimes. Decided via grilling (2026-08-20), as a **separate effort** from `.scratch/core-cleanup/`.

## Decisions

- **Container**: separate effort, own ticket stream (Q14=a).
- **Scope**: the wrapper + its tests only — `node/index.js` → `node/index.ts`, `node/test/smoke.mjs` → `.ts`, `node/test/clean-install.mjs` → `.ts` (Q15=a).
- **Runtime target**: dual-runtime TypeScript — sources type-strip cleanly (`erasableSyntaxOnly`) so they run under **both Node and Bun**; CI matrix unchanged (Node 22/24/26 + Bun 1.3.14). Bun is used for the **local** benchmark and local dev environments (Q16).
- **Typecheck + binding types**: `tsconfig.json` + a `tsc --noEmit` CI step; the native addon is typed by a **hand-written** `node/native.d.ts` (~9 methods) — not `napi build` typegen (Q17=b).
- **Publish strategy**: compile-on-publish — `tsc` emits `dist/index.js` + types; `main` → `dist/index.js`; `files` ships compiled JS + types. Dev/test run the raw `.ts` directly. Safe for plain-Node npm consumers (Q18=a).

## Out of scope

- `benchmarks/js/*`, `bench.mjs`, `shared/config.mjs` — stay `.mjs`, run under Bun locally.
- `integration/*.mjs` — stays as-is.
- No behavior change to the wrapper's runtime semantics; this is a typing + tooling migration.

## Constraint: erasable-only TS

Because Node runs the sources by type stripping (no transform step), the TS must be **erasable-syntax-only**: no `enum`, no `namespace` (with runtime code), no parameter properties. Types, interfaces, and type-only imports are fine.

## Tickets

- `issues/01-...` tooling + native types
- `issues/02-...` wrapper migration + publish strategy (blocked by 01)
- `issues/03-...` test migration (blocked by 02)
- `issues/04-...` CI matrix (blocked by 03)