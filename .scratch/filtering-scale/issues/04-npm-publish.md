# npm publish

Type: task
Status: ready-for-human

## Question

The prebuilt-distribution pipeline is in place (root `spatial-rules` +
per-platform optionalDependencies packages, platform-resolving loader, CI
matrix with publish-on-tags); the remaining operational step is the registry
publish. `spatial-rules` is verified available on npm (unclaimed).

Steps (human — requires registry credentials):
1. Run the CI publish matrix on a version tag (v1 map, ticket 18).
2. Verify clean installs across the platform matrix (`node/test/clean-install.mjs`
   + smoke on non-host platforms).
3. Confirm `npm install spatial-rules` resolves the correct per-platform binary
   and the README install snippet works.

Run: clean-install smoke green on host (and CI matrix); `bun` + `node` smoke
pass through the installed package path.

## Comments

2026-08-20: the published surface changed before release — `query()` now
returns a chainable `QueryResult` (tickets 01–03 landed: point candidates,
whole-clause `$nor`, chainable output with mask/indices/toGeoJson/
toOutcomesJson/summary). The smoke/clean-install verification steps therefore use
`.mask()`. What ships: Buffer-in/mask-out native core, 7 DE-9IM predicates,
property `where` (+`$nor`), point candidates, chainable output, atomic
replace, canonical persistence. Publish from `main`.

2026-08-21: the wrapper API was renamed for clarity (commit `b7c3eef`):
`toMask`→`mask`, `toIndices`→`indices`, `toRichJson`→`toOutcomesJson`,
`toJSON`→`toCanonical`; and unified (commit `431d2b5`): `queryAsync` returns
`Promise<QueryResult>`, `queryOutcomes` removed (use `query().toOutcomesJson()`).
The consumer verification steps above use the current names.
