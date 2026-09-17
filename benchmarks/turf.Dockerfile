# Turf.js HTTP baseline image for the engine-vs-turf memory/throughput
# comparison (benchmarks/js/turf-server.mjs). Builds the JS baseline server
# only — no engine code.
#
#   docker build -f benchmarks/turf.Dockerfile -t turf-baseline .
#   docker run --rm -d --name turf-load -p 3001:3000 turf-baseline
#   bun run bench load --endpoint=raw --base-url=http://localhost:3001
#
# For the memory-capped comparison see docs/benchmarks.md §HTTP serving memory.

FROM oven/bun:1.4.2
WORKDIR /app
COPY benchmarks/js/turf-server.mjs /app/benchmarks/js/turf-server.mjs
COPY shared/config.mjs /app/shared/config.mjs
COPY benchmarks.json /app/benchmarks.json
COPY benchmarks/data/rules.geojson /app/benchmarks/data/rules.geojson
COPY benchmarks/data/candidates.geojson /app/benchmarks/data/candidates.geojson
COPY package.json /app/package.json
RUN bun add @turf/turf@6.5.0 express@4.21.2
EXPOSE 3000
CMD ["bun", "benchmarks/js/turf-server.mjs"]
