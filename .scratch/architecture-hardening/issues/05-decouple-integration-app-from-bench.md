# 05 — Decouple the integration app from the bench harness

Type: task
Status: resolved
Blocked by: None — can start immediately

Origin: 2026-08-19 architecture review, candidate 5.

## What to build

The runtime-facing integration app should not reach into the benchmark harness for its configuration. Today the app imports its config plumbing and query shape from the benchmark layer and takes its defaults from the benchmark config file, so the container image has to copy the harness in just to boot the app, and runtime defaults silently track benchmark defaults.

The app is by design an end-to-end exerciser of the harness, so some coupling is intentional — the scope here is the **config/query-shape** coupling, not the app's existence. Move the shared config plumbing and the query shape into a neutral module that both the harness and the app consume (decide the neutral location as part of the work; it must not be the bench harness), so the app no longer depends on the bench layer for its configuration — the deletion test passes for the config surface: removing the harness's config helpers leaves the app intact.

## Acceptance criteria

- [ ] The integration app no longer imports config plumbing or the query shape from the benchmark harness; it consumes the neutral module
- [ ] Deleting the benchmark harness's config helpers does not break the integration app (deletion test passes for the config surface)
- [ ] Container image builds without copying the harness's config in (Bun-tag pinning is tracked separately — ticket 06)
- [ ] Integration smoke, memory, and load benchmarks still green; behavior identical to today

## Answer

Implemented. Config plumbing + the query shape moved to a neutral
`shared/config.mjs`; `integration/{server,memory,smoke}.mjs` import it directly,
and `benchmarks/js/common.mjs` re-exports it for harness consumers. The
Dockerfile copies `shared/config.mjs` instead of the harness. Deletion test
passes: the app no longer references the harness for configuration. Memory
harness and smoke verified green.
