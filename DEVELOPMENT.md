# Development

This document is for contributors building, testing, and benchmarking the
repository locally. For the consumer-facing API see [README.md](README.md); for
contribution process see [CONTRIBUTING.md](CONTRIBUTING.md).

## Repository layout

```
core/        pure-Rust engine (Ruleset, Engine, query pipeline, indexes)
node/        napi-rs addon + JS wrapper + per-platform npm packages
benchmarks/  criterion algorithm ladder + turf.js baseline + dataset + memory scaling harness
integration/ Bun + Express app + Docker image + memory harness
docs/        CONTEXT.md, Initial-plan.md, benchmarks.md, adr/
```

## Build & test

```bash
# Rust core + binding
cargo test --workspace
cargo clippy --workspace --all-targets

# Node/Bun binding smoke (build the addon first)
cargo build -p spatial-rules-node
# Windows: copy target/release/spatial_rules_node.dll -> node/spatial_rules.node
# Linux:   copy target/release/libspatial_rules_node.so -> node/spatial_rules.node
cd node && npm install && npm run typecheck
node --experimental-strip-types test/smoke.ts   # flag needed on Node 22.6+, default-on later
bun  test/smoke.ts

# Benchmarks + integration — one dispatcher at the repo root
bun install                        # once: harness deps (turf, rbush, express)
bun run bench                      # list every command
bun run bench build                # build binding (+ copy) + cross_check binary
bun run bench cross-check && bun run bench perf
bun run bench memory-scale            # scaling & lifecycle memory grid
bun run bench all                  # full battery

# Docker integration (server + memory measurement)
docker build -f integration/Dockerfile -t spatial-rules .
docker run --rm --memory=128m -p 3000:3000 spatial-rules
```

## Configuration

The benchmark and integration harnesses read all configuration from the single
committed `benchmarks.json` at the repo root; per-run tweaks are `--flag=value`
arguments (e.g. `bun run bench crossover --sizes=20,200,1000,5000`). There are
**no environment variables and no `.env` files** — every knob is either in
`benchmarks.json` or passed as a flag. See
[`docs/benchmarks.md`](docs/benchmarks.md) for the full key → flag map.

The core engine and the `node/` addon read no configuration at all; their
input travels through the API only.

## Architecture docs

- [`CONTEXT.md`](../CONTEXT.md) — domain glossary (single source of vocabulary).
- [`docs/Initial-plan.md`](../docs/Initial-plan.md) — implementation spec.
- [`docs/adr/`](../docs/adr/) — architecture decision records.
- [`docs/benchmarks.md`](../docs/benchmarks.md) — perf and memory evidence.
