# 06 — Pin the Docker runtime Bun tag to CI's version

Type: task
Status: resolved
Blocked by: None — can start immediately

Origin: 2026-08-19 architecture review (split out of candidate 5 — the decoupling ticket 05).

## What to build

The runtime image and CI should run the same Bun. Today the Dockerfile uses a floating minor tag (`oven/bun:1.3`) while CI pins an exact version (1.3.14), so the image Bun version can silently roll forward and memory/load numbers stop being reproducible across image rebuilds. Pin the Dockerfile to the exact Bun version CI uses.

## Acceptance criteria

- [ ] Dockerfile uses the exact Bun tag matching CI (1.3.14)
- [ ] Image builds and the integration smoke passes with the pinned tag
- [ ] Memory/load benchmark numbers are reproducible across image rebuilds

## Answer

Implemented. `integration/Dockerfile` pins `FROM oven/bun:1.3.14`, matching
CI's `oven-sh/setup-bun@v2` `bun-version: 1.3.14`. Local Bun is also 1.3.14.
