# Bun + Express + Docker integration test

Type: task
Status: claimed
Blocked by: 16

## Question

Phase 7 integration — a real Bun + Express app embedding the addon in Docker (execution on the map):

- Small Express API (e.g. `/query`) that loads the addon, holds a ruleset, and queries ~1,000 candidate footprints.
- Dockerfile running the prebuilt `linux-x64-gnu` (and/or musl) addon; verify it loads and runs inside the container.
- Smoke test asserting the production flow (candidate footprints + rules → mask).

The Docker image runs the app with the addon working.

## Comments

### 2026-08-18 — integration app + Dockerfile, locally verified (Docker run pending)

- **App** (`integration/server.mjs`): Bun + Express server embedding the addon; `GET /health`, `POST /query` (candidates → mask), `POST /replace` (observability). Loads the 30-rule dataset from `benchmarks/data/rules.geojson`.
- **Smoke** (`integration/smoke.mjs`): posts 1,000 candidate footprints and asserts the mask shape — **passed locally** (`bun server.mjs` + `node smoke.mjs` → 1,000 candidates, 481 matched).
- **Dockerfile** (`integration/Dockerfile` + root `.dockerignore`): multi-stage — `rust:1.96-slim-bookworm` builds the `linux-x64-gnu` addon; `oven/bun:1.3` runs it.
- **Remaining**: `docker build -f integration/Dockerfile -t spatial-rules .` then `docker run --rm -p 3000:3000 spatial-rules`. Blocked here because the Docker daemon isn't running in this environment — run it once Docker Desktop is up (or in CI).

Status stays `claimed` until the image builds and runs.

