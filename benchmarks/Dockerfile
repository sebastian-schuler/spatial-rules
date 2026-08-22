# Reproducible Linux memory-benchmark image (memory-benchmark tickets 01-03).
#
# Builds the release `memory_scaling` binary and the node addon for linux-gnu,
# then runs the harnesses under the pinned Bun (oven/bun:1.3.14,
# architecture-hardening 06) so the numbers match what a deployed container
# sees. All measurements report process-level RSS from `/proc/self/status`
# (VmRSS/VmHWM) — the Linux ground truth the Windows runs could not sample.
#
#   docker build -f benchmarks/Dockerfile -t spatial-rules-bench .
#   docker run --rm spatial-rules-bench memory-scale            # full default grid
#   docker run --rm spatial-rules-bench memory-scale-probe      # 50-swap 100k x 100 probe
#   docker run --rm spatial-rules-bench memory-turf             # engine vs turf footprint
#   docker run --rm spatial-rules-bench memory                  # container baseline (65 MB / 128 MB)
#
# `memory-scale-probe` runs the binary directly (single-cell mode, which does
# not apply the aggregate run's wall-time swap cap) with 50 replacements:
#
#   docker run --rm spatial-rules-bench target/release/memory_scaling \
#       --cell=100000,100 --replacements=50 --candidates=1000 --query-batches=20

FROM rust:1.96-slim-bookworm AS build
WORKDIR /app
COPY . .
RUN cargo build --release -p spatial-rules-benchmarks --bin memory_scaling \
    && cargo build --release -p spatial-rules-node

FROM oven/bun:1.3.14
WORKDIR /app
# `target/` and `*.node` are dockerignored, so the repo copy below brings no
# binaries; the release artifacts are placed here *after* it so they win.
COPY . .
COPY --from=build /app/target/release/memory_scaling /app/target/release/memory_scaling
COPY --from=build /app/target/release/libspatial_rules_node.so /app/node/spatial_rules.node
RUN bun install
CMD ["bun", "bench.mjs"]