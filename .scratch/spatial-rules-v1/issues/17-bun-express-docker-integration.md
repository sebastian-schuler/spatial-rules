# Bun + Express + Docker integration test

Type: task
Status: open
Blocked by: 16

## Question

Phase 7 integration — a real Bun + Express app embedding the addon in Docker (execution on the map):

- Small Express API (e.g. `/query`) that loads the addon, holds a ruleset, and queries ~1,000 candidate footprints.
- Dockerfile running the prebuilt `linux-x64-gnu` (and/or musl) addon; verify it loads and runs inside the container.
- Smoke test asserting the production flow (candidate footprints + rules → mask).

The Docker image runs the app with the addon working.
