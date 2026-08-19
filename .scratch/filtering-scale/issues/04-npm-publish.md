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
