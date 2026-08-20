# Multi-stage image for the Bun + Express integration app.
#
#   docker build -f integration/Dockerfile -t spatial-rules .
#   docker run --rm -p 3000:3000 spatial-rules
#
# Stage 1 builds the native addon for linux-x64-gnu; stage 2 runs it under Bun.

FROM rust:1.96-slim-bookworm AS build
WORKDIR /app
COPY . .
RUN cargo build --release -p spatial-rules-node

FROM oven/bun:1.3.14
WORKDIR /app
COPY --from=build /app/target/release/libspatial_rules_node.so /app/node/spatial_rules.node
COPY node/index.ts /app/node/index.ts
COPY node/package.json /app/node/package.json
COPY integration/server.mjs /app/integration/server.mjs
COPY integration/memory.mjs /app/integration/memory.mjs
COPY integration/smoke.mjs /app/integration/smoke.mjs
COPY shared/config.mjs /app/shared/config.mjs
COPY benchmarks.json /app/benchmarks.json
COPY package.json /app/package.json
COPY benchmarks/data/rules.geojson /app/benchmarks/data/rules.geojson
COPY benchmarks/data/candidates.geojson /app/benchmarks/data/candidates.geojson
RUN bun install --production
EXPOSE 3000
CMD ["bun", "integration/server.mjs"]
