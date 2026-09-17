# 01 — HTTP-serving performance & memory: primary-source research (2026-09-17)

Research only; no code changed. Scope: this engine behind an HTTP API
(Express/Node), traced through the reference deployment `integration/server.mjs`
+ `integration/memory.mjs` and the measured numbers in `docs/benchmarks.md`
(§3, §3b, §HTTP serving memory).

Sources are primary: repo code/ADRs (with `file:line`), the Node.js API docs,
libuv docs, Express docs, napi-rs docs, the Linux cgroup-v2 kernel doc, and
`malloc(3)`. URLs are inline. Where the repo's own docs disagree with the
current upstream docs, that is flagged explicitly rather than smoothed over.

**Baseline being reasoned about** (docs/benchmarks.md:29–33, 571–604, 758–823):

| Metric | Value |
|---|---|
| 1,000 cand × 30 rules, unfiltered mask | ~15 ms core / 18.5 ms addon |
| Production query (`where{classification}` + 2 exclusions) over HTTP, 1 shot | 22.3 ms/request |
| Single-process sustained ceiling, raw bytes | ~165 req/s (⇒ ~6.1 ms CPU/request) |
| Single-process sustained ceiling, JSON in/out | ~130 req/s |
| HTTP process `VmHWM` / cgroup `memory.peak` @ 128 MB cap | 138–149 MiB / ~119 MiB |
| In-process peak (no socket) | ~65–67 MiB |

Dataset: `benchmarks/data/candidates.geojson` = 303,450 B (~0.29 MiB, 1,000
features); `rules.geojson` = 595,135 B (30 rules). So one candidate batch is
~296 KiB on the wire — small; the numbers below are dominated by per-request
transient churn, not by the ruleset.

---

## 1. Where a request does the same work twice

Two request paths exist. The `/query` path is the naive one; `/queryRaw` is the
optimised one and the docs already measure the gap.

### `/query` (JSON in/out) — `integration/server.mjs:36,42–57`

```
36: app.use(express.json({ limit: '20mb' }));
43: const { candidates, query, rich } = req.body ?? {};
48: const queryJson = JSON.stringify(query ?? SPATIAL_QUERY);
50: const result = ruleset.query(Buffer.from(JSON.stringify(candidates)), queryJson);
51: const body = { mask: Array.from(result.mask()) };
52: if (rich) body.outcomes = JSON.parse(result.toOutcomesJson());
53: res.json(body);
```

Materialisations/copies of the candidate batch on this path:

1. `express.json` buffers the body and builds a **JS object graph** (the parsed
   GeoJSON tree) — line 36.
2. `JSON.stringify(candidates)` re-serialises that graph to a **JS string**
   — line 50.
3. `Buffer.from(string)` **copies** that string into a Buffer — line 50
   (`Buffer.from(string)` allocates/copies; Node Buffer docs).
4. `node/index.ts:162–169` (`toGeoJsonBuffer`) would copy again for the
   object/string input shapes, but here it receives a Buffer and passes it
   through byte-faithfully (`if (Buffer.isBuffer(value)) return value;`).
5. Rust parses the bytes into a `geojson::GeoJson` DOM and then copies each
   coordinate into a `geo::Geometry` — `core/src/runtime/ingestion.rs:73–81`,
   `:55–59`; i.e. **two representations inside Rust**, the DOM living until the
   features drop (perf-memory 08 item 2, measured and **wontfix** at the 1k
   shape).
6. The mask: `Engine::query_mask` → `Vec<u8>` (`core/src/runtime/engine.rs:74`),
   handed out via napi `Uint8Array::from(Vec<u8>)` (`node/src/lib.rs:79`). On
   runtimes that allow external buffers napi-rs transfers the allocation
   **without a copy** (napi-rs Typed Array docs, "Buffer … transfers the
   allocation … without a copy").
7. Response: `Array.from(result.mask())` builds a **JS array of 1,000 numbers**
   (line 51), then `res.json` **JSON.stringify**s it (line 53; Express docs:
   `res.json` sends `JSON.stringify(param)`).
8. Rich only: `toOutcomesJson()` returns a JSON **string** (streamed in Rust,
   no DOM — `bindings-common/src/lib.rs:265–282`, landed as perf-memory 07),
   then line 52 **`JSON.parse`s it back into a JS object** so `res.json` can
   **stringify it again**.

So the JSON path materialises the candidate batch roughly **four times** (JS
object tree → JS string → Buffer copy → Rust GeoJSON DOM + `geo` geometry), and
the rich path round-trips an already-serialised JSON string through
parse+stringify.

### `/queryRaw` (bytes in/out) — `integration/server.mjs:84–96`

```
84: express.raw({ type: 'application/octet-stream', limit: '20mb' })
90: const mask = ruleset.query(req.body, queryJson).mask();
92: res.send(Buffer.from(mask));
```

One incoming Buffer, the Rust DOM + `geo` geometry (unavoidable, step 5), the
mask `Vec<u8>`→`Uint8Array`, then `Buffer.from(mask)` — which **copies** the
1 KB mask (Node Buffer docs: `Buffer.from(arrayBuffer)` is a view; here the
argument is a `Uint8Array`, treated as array-like and copied) — and `res.send`
sets `Content-Length` (Express docs). No JS object tree, no stringify.

### What the measurements already say

`docs/benchmarks.md:803–804, 808–812`:

- Skipping `.json()` (raw bytes in/out) is worth **~25–28% more throughput**
  (130 → 165 rps) and **~7 ms lower p50** at low concurrency. At saturation
  that is ~1.6 ms less event-loop CPU per request (1/130 − 1/165).
- At c=5: raw p50 30.5 ms vs JSON p50 37.7 ms (docs:783–794).
- The unfiltered ~15–20 ms ladder case is *not* the production query: the
  `where` equality index prunes rules, so the reference query is faster
  (docs:810–812). The raw ceiling implies ~6.1 ms CPU/request.

**Conclusion for Q1.** The duplicate work is almost entirely on the JS side of
the boundary and is already quantified: `express.json` parse + re-`stringify` +
`Array.from` + `res.json`. The Rust side does hold the GeoJSON DOM and the
`geo` geometry at once, but at 0.29 MiB input that transient was measured
(~0.2 MiB DOM) and deliberately left alone (perf-memory 08). Do not re-propose
the ingestion rewrite.

---

## 2. The deferred native `filteredGeojson(candidates, query) -> String`

### Status: already deferred, twice

- ADR-0014 (`docs/adr/0014-chainable-query-result.md:5`): "*a native
  filtered-geojson method (would beat the JS re-parse at very large scales —
  **deferred**; the JS view is the chosen tradeoff at the ~1k production
  scale)*".
- `.scratch/post-v1/spec.md:27–34` lists it under **"Open proposals (deferred,
  not ticketed)"**, `filteredGeojson(candidates, query) -> String`, "**Most
  likely to be ticketed** if the endpoint returns the filtered
  FeatureCollection." Trigger: the real endpoint's response contract.
- `.scratch/filtering-scale/issues/03-filter-return-shapes.md:43–44`: the
  response-contract question "became moot: the caller picks the view at the
  call site." No ticket exists.

Neither ADR-0014 nor the spec mentions the blocker below, which is the part
worth recording.

### What the JS view actually does, and the blocker

`node/index.ts:284–295` (`toGeoJson`):

```ts
const raw = this._candidates.toString('utf8');
const parsed = JSON.parse(raw);
const features = parsed.type === 'FeatureCollection' ? parsed.features : [parsed];
const kept = [];
for (let i = 0; i < this._mask.length; i += 1) if (this._mask[i] === 1) kept.push(features[i]);
return JSON.stringify({ type: 'FeatureCollection', features: kept });
```

Properties are preserved **because the wrapper retains the original candidate
Buffer** (`node/index.ts:213–218`, ADR-0014:3: "properties preserved from the
original payload — no lossy round-trip through the engine's parsed
candidates"). The engine's `Candidate` carries **only `id` + geometry**
(`core/src/model/candidate.rs:19–33`); candidate properties are never parsed.
So a native `filteredGeojson` cannot re-serialise from the parsed candidates
without a new "retain candidate properties" design — which is exactly the
change perf-memory 10 declined as a breaking/behavioural contract change, and
which ADR-0020 scopes out of the wire layer.

The only way to keep the current property-faithful contract natively is to
**preserve the original feature bytes**: parse the input into raw feature
slices (serde_json's `RawValue`/`Box<RawValue>` with the `raw_value` feature),
filter by the mask index, and concatenate the slices into a
`FeatureCollection` string. That is zero DOM on either side.

### Rough saving

| Shape | Input | JS view today | Native raw-slice view | Verdict |
|---|---|---|---|---|
| 1,000 cand (production) | 303,450 B | `JSON.parse` + filter + `JSON.stringify` of ~296 KiB object graph; order ~1–3 ms | Rust parse to index (~same as the query's existing parse) + string concat; skips the JS graph and the JS re-serialise | Real but small next to the ~6 ms production query / ~15 ms unfiltered query. Only pays if the endpoint returns filtered GeoJSON |
| 100,000 cand | 28.9 MiB GeoJSON | JS object graph (hundreds of MiB) + seconds | Rust raw slices (~tens of MiB), sub-second | Large — **but 100k candidates is explicitly not a deployment shape** (perf-memory 08 item 2; crossover bench 100k = 456 ms addon, docs:907–910) |

The requested "raw request-body Buffer in, filtered FeatureCollection string
out, pass-through `res.send`" (`post-v1/spec.md:29`) would therefore skip the
JS object graph and the response re-serialisation, but **not** the Rust parse
(the query already parses), and it needs the `RawValue` splice design plus the
`raw_value` serde feature. At 1k it is ~1–3 ms; at 100k it is large but the
shape is out of scope. **Keep it deferred; the trigger is the endpoint
contract, not a perf ceiling.** If it is ever built, build the raw-slice
version — a version that re-serialises from parsed candidates would need
candidate properties retained and would silently drop them otherwise.

---

## 3. Sync vs async, and the pool model

### How a sync native call interacts with Express

Node runs JS callbacks on the single event loop; a synchronous native call is
one callback, so while `query()` runs (~6 ms production / ~15 ms unfiltered),
all other clients and the loop are blocked (Node "Don't Block the Event Loop":
"*if the Event Loop spends too long at any point, all current and new clients
will not get a turn*"). The repo already measured this: under load `/health`
latency ≈ query latency, and it is CPU-bound at ~165 rps raw (docs:798–807).

### libuv threadpool: the documented number, and the correction

- libuv: default pool size **4**, settable via `UV_THREADPOOL_SIZE` *at
  startup*, absolute max **1024**; the pool is **global and shared** across all
  event loops; libuv preallocates the max number of threads when first used
  (libuv `threadpool` docs).
- Node uses that pool for fs, DNS (`dns.lookup`/`lookupService`), crypto
  (`pbkdf2`/`scrypt`/`randomBytes`…) and zlib (Node "Don't Block the Event
  Loop", "What code runs on the Worker Pool?").

**Correction worth acting on:** napi-rs 3.x documents that `#[napi] async fn`
runs on **the NAPI-RS-managed Tokio runtime**, while `AsyncTask` (the `Task`
trait) is what runs on **libuv's** threadpool. The decision table in the
napi-rs "Async and concurrency" guide says: *"`#[napi] async fn` → NAPI-RS
Tokio runtime"* and *"`AsyncTask<T>` → libuv thread pool"*. `node/Cargo.toml:13`
enables `napi = { version = "3", features = ["napi8", "async"] }` and
`Cargo.lock:760` pins **napi 3.12.1**; `async` is the feature that enables the
Tokio runtime ("async fn" docs).

Consequence: the repo's claim in ADR-0009's amendment and
`.scratch/post-v1/issues/06-query-async.md:19` that `queryAsync` runs "on
libuv's threadpool (default 4 threads, `UV_THREADPOOL_SIZE`)" and contends
with fs/DNS/crypto/zlib is **out of date for napi-rs 3.x**. `UV_THREADPOOL_SIZE`
does not size the `queryAsync` pool; the Tokio runtime's worker count does
(settable only by installing a custom runtime — napi-rs `create_custom_tokio_runtime`
/ `napi-async-runtime` `RuntimeOptions`; the default worker count is not stated
in the docs and should be measured). This does **not** change the shipped
behaviour or the copy (see below) — it changes what tuning to reach for and
what "contention" means.

I could not find a primary napi-rs doc stating the default Tokio worker count,
so treat "how many parallel `queryAsync` calls actually run" as an
**unverified** assumption until measured.

### The async path's real costs (from the code)

- One candidate-Buffer copy per call: `node/src/lib.rs:94,123`
  `candidates.as_ref().to_vec()`. napi-rs now documents `Buffer` as
  `Send + Sync` (Typed Array docs), so the ADR-0009 wording "buffers are not
  moved across threads" no longer holds; the copy is still the safe choice
  because napi-rs explicitly warns that `Send + Sync` "do not synchronize the
  shared bytes" and a worker reading while JS may mutate is a data race. At
  296 KiB the copy is well under ~0.1 ms at memory bandwidth.
- Per-thread prepared-geometry memo: each runtime worker prepares its own
  copy of the rules it touches (ADR-0010; `core/src/indexing/prepared_cache.rs`).
  Bounded, but a first-touch latency tail per worker.
- Promise/dispatch overhead per call.
- `toOutcomesJson()`/`toJson()` on an async result make a **synchronous** rich
  call on first use — there is no async rich path
  (`node/index.ts:306–311, 377–382`; ADR-0014 amendment). So an async endpoint
  that also returns rich blocks the loop for the rich call anyway.

### Is there a batch-size threshold where async wins?

Reasoning, not measured (the repo has no async endpoint and `bench load` only
drives sync `/query`, `/queryRaw`):

- Async removes blocking of the loop (always a win for co-located work like
  `/health`) and can *parallelise* across runtime workers, so throughput can
  rise toward min(cores, workers) × single-thread.
- It adds a fixed dispatch + copy overhead (call it ~0.1–0.3 ms at this batch).
- At the production shape (~6 ms) that is a few percent; at the unfiltered
  ~15 ms it is ~1–2%. Below roughly **~1 ms of query work** (order 100–200
  candidates at this shape) the fixed overhead dominates and sync is both
  lower-latency and lower-overhead.
- On a single-core container there is nothing to parallelise, so async is
  strictly overhead; it only buys loop responsiveness.
- `queryAsync` is not currently exercised by any harness — a threshold claim
  needs a `bench load` A/B with an async endpoint. Until then, the honest
  statement is: **async helps event-loop headroom at any size once queries are
  in flight, and helps throughput only when there are ≥2 workers and ≥2
  in-flight queries whose work is ≳1 ms.**

---

## 4. worker_threads / cluster: memory math vs throughput

### Per-worker state

- Worker threads run in parallel and each gets **its own JS engine instance /
  event loop**; they share memory only by transferring `ArrayBuffer`s or
  sharing `SharedArrayBuffer`s. Unlike `child_process`/`cluster`, they *can*
  share memory (Node `worker_threads` docs, introduction; `new Worker`
  `resourceLimits` "an optional set of resource limits for the **new JS engine
  instance**"). So each worker that runs `new SpatialRuleset(rules)` builds its
  own Rust ruleset + per-thread prepared memo. Node-API addons may be loaded
  from multiple environments (Node addons "Worker support"), and the native
  image/static state is per process — only the `thread_local` memo is
  per-thread. The ruleset is per worker because the JS wrapper constructs one.
- `cluster` is child processes: the whole Node/Bun runtime **and** the ruleset
  duplicate, with no shared memory at all. Strictly worse than
  `worker_threads` on both axes.

### The math at the documented scales

Ruleset bytes/rule after perf-memory 02–04 (`docs/benchmarks.md:317–336`):

| Scale | Steady ruleset | bytes/rule |
|---|---|---|
| 30 rules (production) | « 1 MiB of data | (fixed overhead dominates) |
| 100,000 × 10 verts | **77.8 MiB** | 0.82 kB |
| 100,000 × 100 verts | ~258 MiB | ~2.6 kB |

- **Production (30 rules):** the ruleset data is negligible; the multiplier is
  the per-worker runtime/isolate and the warmed prepared memo. N workers cost
  roughly `N × (isolate + small ruleset + small memo)` plus the shared image;
  the in-process serving peak is ~65–67 MiB and HTTP ~119–149 MiB per process
  (docs:571–604). On a multi-core box, `worker_threads`/`cluster` is a clear
  throughput win (up to ~N× the ~165 rps single-thread ceiling).
- **100k rules:** 4 workers ≈ `4 × 78 MiB ≈ 312 MiB` **of ruleset alone**
  (100k×10), before per-worker isolates/memos — under a 128–256 MB cap that
  does not fit. At 100k×100 it is ~1 GiB. Use **one process + `queryAsync`**
  instead: async workers share the single `Arc<Engine>`/`Ruleset` (the lazy
  thread-local memo is the only per-thread state, docs:606–611), so you get
  N compute threads for one ruleset's memory.
- **When it isn't worth it:** single/low core count; a large ruleset under a
  memory cap; or when the loop already has spare capacity (you are
  latency-bound, not CPU-bound).

### Operational caveats

- `replace()` must be broadcast to every worker; each worker rebuilds
  independently, so the replacement peak (old + new coexist, docs:418–421) is
  paid per worker, and briefly on every worker at once if you fan out.
- napi-rs explicitly warns that worker lifecycle under **Bun** is not the same
  as Node and that abrupt termination during async native work has an open
  crash report (napi-rs "Async and concurrency", "Worker shutdown protocol").
  The reference image runs Bun (`integration/Dockerfile:13`), so test the exact
  runtime before relying on worker_threads there.
- Per-worker V8 heaps can be bounded with `resourceLimits.maxOldGenerationSizeMb`
  (worker_threads docs) — useful under a cgroup cap so a worker GCs rather than
  triggering a process-wide OOM.

---

## 5. Per-request memory, GC, and `VmHWM` vs cgroup peak

### Transients per request

- `/query`: express.json object tree, `JSON.stringify` string, `Buffer.from`
  copy, Rust `geojson::GeoJson` DOM + `Vec<Candidate>` (`geo` geometry), mask
  `Vec<u8>`→`Uint8Array`, `Array.from(mask)` JS array, `res.json` string.
  Rich adds `Vec<CandidateOutcome>` + the streamed JSON string.
- `/queryRaw`: incoming Buffer, Rust DOM + `Vec<Candidate>`, mask
  `Vec<u8>`→`Uint8Array`, `Buffer.from(mask)` copy, `res.send`.
- The result views are one-shot and uncached by design; results are
  "short-lived (one request → GC)" (ADR-0014 memory contract;
  `filtering-scale/issues/03:27–32`).

The in-process harness peaks at ~65–67 MiB but HTTP reaches ~138–149 MiB
`VmHWM` / ~119 MiB cgroup (docs:571–604). The driver is per-request churn, not
queueing: a 1/5/10/25 concurrency sweep still peaked 126/143/152/138 MiB, so
even a **serialized** server reaches ~126 MiB (docs:586–588). That is the
transient object graph + strings + Rust representations churning at ~675 req/s.

### `VmHWM` vs cgroup `memory.peak`

- Kernel cgroup-v2 docs: `memory.current` = current charge; `memory.peak` =
  "the max memory usage recorded for the cgroup and its descendants"; `memory.max`
  = the hard limit that invokes the OOM killer; `memory.events` reports
  `max`/`oom`/`oom_kill` (Linux cgroup-v2, Memory Interface Files).
- `VmHWM` is process RSS, which includes file-backed/reclaimable pages (the
  addon `.so`, the Bun/Node binary and its mappings) that the kernel can evict
  without OOM. The cgroup charge excludes what it can reclaim, hence the
  ~20–30 MiB gap.
- **Actionable:** size K8s `limits.memory` off the **cgroup `memory.peak`**
  (192–256 MB with headroom), not `VmHWM` and never the in-process ~67 MB —
  already the docs' conclusion (docs:594–597; architecture-hardening 09).
  Additionally:
  - Bound the JS heap below the cgroup limit (`--max-old-space-size`, Node CLI;
    per-worker `resourceLimits.maxOldGenerationSizeMb`) so V8/JSC GCs first.
  - For a multithreaded process (async Tokio workers or worker_threads), glibc
    creates a malloc arena per contending thread (`malloc(3)`, NOTES: "glibc
    creates additional memory allocation arenas if mutex contention is
    detected"); `MALLOC_ARENA_MAX`/mallopt tunables can cap the resulting RSS
    fragmentation. *Medium confidence; measure — this is an ops knob, not a
    code change.*
  - `memory.events` should stay all-zero (`max`/`oom`/`oom_kill`) as the
    "survives" assertion; the existing harness already reads it
    (`docs/benchmarks.md:561–563`).

---

## 6. Result delivery ergonomics

Per ADR-0014:3 and `node/index.ts`:

| View | Native crossings | JS allocation | Notes |
|---|---|---|---|
| `mask()` | **1** (`query()`) | none extra | `Uint8Array`, 1 B/candidate (`index.ts:226–228`) |
| `indices()` / `invalidIndices()` | **1** | one exactly-sized `Uint32Array` (native-side? no — pure JS two-pass over the mask, `index.ts:236–267`) | 4 B/matched; directly usable to slice the candidate array |
| `count()` / `summary()` | **1** | none | pure JS scan of the mask |
| `toGeoJson()` | **1** | JS object graph + string (`index.ts:284–295`) | properties need the retained buffer |
| `toOutcomesJson()` / `toJson()` | **2** (`query` + rich) | rich JSON string, then whatever the caller does with it | lazy + cached; sync even on async results |

- **Fewest crossings:** `query()` alone. Everything cheap is derived from the
  captured mask in JS; the rich view is the only extra crossing, and it is
  lazy so mask-only callers never pay it (ADR-0014).
- **Fewest JS allocations for a keep/drop endpoint:** if the response is the
  mask, return the raw bytes — `res.send(Buffer.from(mask))` — not
  `Array.from(mask)` (which allocates 1,000 JS numbers) + `res.json`. The
  measured raw-vs-JSON gap (25–28% throughput, docs:803–804) includes the
  request side, but the response side is pure avoidable work.
- **If the endpoint needs per-feature decisions:** `indices()` is the right
  primitive (exactly-sized, no invalid ambiguity if you also read
  `invalidIndices()`), and it is one crossing. For a 481/1,000 match that is
  ~1.9 KB vs a 1 KB mask — both negligible on the wire; pick by how the caller
  consumes it.
- **Rich JSON string:** if you already hold it, do not `JSON.parse` it and
  `res.json` it back (the reference server does — `server.mjs:52,74`). Send the
  string directly.

---

## 7. Other HTTP-specific findings

- **`res.json` vs a pre-serialised string vs `res.send`.** `res.json` is
  `JSON.stringify` + JSON content type (Express docs). `res.send(Buffer|String)`
  "automatically assigns the `Content-Length` response header" (Express docs) —
  good for keep-alive; `res.send` of a String sets `text/html` unless
  overridden, so use `res.type('application/json').send(str)`. `res.end(str)`
  skips Express's send path entirely. For the rich view, feeding the already
  serialised string avoids the round trip.
- **Mask wire format.** The reference `/query` returns `{ mask: number[] }`
  (up to ~4 KB of JSON for 1,000 candidates) where `/queryRaw` returns 1,000
  bytes. A compact string or base64 would sit between them, but the measured
  win already belongs to the raw endpoint. A bit-packed mask (125 B) is not
  worth a wire-contract change at this size.
- **Compression.** `compression` uses zlib (and brotli), which Node runs on the
  **libuv threadpool** (compression README + Node zlib docs); default
  `threshold` is 1 KB and it will not compress `Cache-Control: no-transform`.
  For 1 KB masks the CPU/negotiation overhead is not worth it; for large
  filtered GeoJSON it can be, but it buffers the body and complicates
  `Content-Length`. Not currently used by `integration/server.mjs`.
- **Keep-alive.** Node defaults: `server.keepAliveTimeout` **5000 ms**,
  `keepAliveTimeoutBuffer` **1000 ms**, `headersTimeout` = min(requestTimeout,
  60000), `requestTimeout` **300000 ms** (Node `http` docs). Raise
  `keepAliveTimeout` above the upstream proxy/LB idle timeout to avoid
  `ECONNRESET`; enable `http.Agent({ keepAlive: true })` on clients (Node http
  docs). `fetch`/undici (used by the load harness) pools by default.
- **`maxRequestsPerSocket`** defaults to 0 (unlimited); setting a threshold
  makes the server drop further requests with **503** (Node http docs) — do not
  set it as a memory lever.
- **Body limit.** `express.json({ limit: '20mb' })` / `express.raw({ limit:
  '20mb' })` (`server.mjs:36,84`) is a sound DoS bound; the engine is
  whole-buffer by design so streaming input buys nothing (ADR-0014:7).
- **Per-request query parse.** `parse_query` builds a `serde_json::Value` DOM
  per request (`bindings-common/src/lib.rs:27–31`, `parse_inputs`). The query
  is tiny and usually constant; caching the parsed `Query` per (ruleset
  version, query JSON) would remove a micro-allocation, but it is far below the
  ~6 ms query — low priority.
- **CPU limits.** The engine is CPU-bound on the JS thread; K8s CPU `limits`
  are CFS throttles, so `limits.cpu` below `requests.cpu` (or below the number
  of cores you actually use) can add latency spikes. Prefer `requests` = cores
  and no explicit CFS ceiling, or a ceiling ≥ requests (cgroup-v2 CPU
  controller).
- **Runtime note.** The reference image is Bun (`oven/bun:1.3.14`,
  `integration/Dockerfile:13`), while most of this guidance targets Node.
  The napi/Tokio/worker caveats above apply the same; Bun's worker lifecycle is
  explicitly called out as different by napi-rs.

---

## Already rejected or deferred — do not re-propose

Check these before ticketing anything above.

| Candidate | Where decided | Why |
|---|---|---|
| Native `filteredGeojson` (and `filteredFeatures`, object `queryRich`, `keep` indices) | ADR-0014:5; `.scratch/post-v1/spec.md:25–34` | **Deferred**, gated on the endpoint's response contract; JS view is the chosen 1k tradeoff |
| Only-method-per-format return shapes | ADR-0014:5; `filtering-scale/issues/03` | Rejected in favour of the chainable `QueryResult` (compute once, format many) |
| Flipping the API to async | ADR-0009 | Rejected; latency gate (p95 > 50 ms) not triggered; `queryAsync` is opt-in |
| `UV_THREADPOOL_SIZE` sizing for `queryAsync` / libuv contention claim | ADR-0009 amendment; `post-v1/issues/06` | Based on napi `async fn` = libuv; **out of date for napi 3.12.1** (Tokio runtime). Flag, don't silently "fix" code |
| Single-representation ingestion (drop the GeoJSON DOM) | `.scratch/perf-memory/issues/08` item 2 | **Wontfix**: premise false (geo serde is not GeoJSON) and the transient is ~0.2 MiB at 1k; the streaming variant would need a hand-written deserializer for an invisible win |
| Drop `Candidate.id` / retain candidate properties | `.scratch/perf-memory/issues/10` item 2 | **Wontfix**: id-less candidates are a behavioural + breaking change (~1% at 100k) |
| Remove the async candidate-Buffer copy | `.scratch/perf-memory/issues/10` item 1 | **Wontfix**: parse is the expensive part; moving it to the JS thread re-blocks the loop |
| Shared cross-thread `PreparedGeometry` | ADR-0010; perf-memory spec non-goals | geo's `PreparedGeometry` is `!Sync`; the thread-local memo is the design, not a placeholder |
| `Ruleset.envelopes` by-id copy removal | `.scratch/perf-memory/issues/08` item 1 | **Keep**: it is the O(1) by-id accessor, not a redundant copy |
| `ReplaceReport` DOM | `.scratch/perf-memory/issues/10` item 3 | **Leave**: 4 fields, control-plane only |
| Rich-JSON DOM → streaming | `.scratch/perf-memory/issues/07` | **Already landed**: `query_rich_json` −79%, `resolve_rich_json` −83% |
| `toOutcomesJson()` returns a string (vs JS objects) | `.scratch/post-v1/spec.md:31` | Deferred object variant; all bindings put a `String` on the wire (ADR-0020) |

Also already well optimised, and not to be "improved": prepared geometry (B→E
≈31×), lazy per-rule preparation, the R*-tree, the equality-index `where`
pruning, the streaming rich serializer, the byte-oriented mask, and the single
native crossing for all cheap views. The request path's remaining cost is
overwhelmingly JS-side transients and per-request churn, not the engine.

---

## Recommendations

Ranked by **(expected effect × confidence) ÷ (effort × risk)**.

### 1. Adopt the raw-bytes shape for the hot endpoint (or make it the documented production path)

- **Effect: high.** The repo already measured it: raw bytes in/out = **25–28%
  more throughput** (130→165 rps) and **~7 ms lower p50** at low concurrency,
  ~1.6 ms less event-loop CPU/request at saturation (docs:798–812). This is the
  single largest measured HTTP lever in the repo.
- **Confidence: high** (measured; mechanism understood — `express.json` parse,
  re-`stringify`, `Array.from`, `res.json`).
- **Effort: low** if the action is "document/serve `/queryRaw` as the
  production contract" and keep `/query` for convenience; **medium** if the
  envelope must stay (`{candidates, query}`) — then use a body parser that
  keeps the raw buffer and a side channel for the query.
- **Risk: low–medium** (client wire-format change; mask bytes vs number array).
- **First step:** decide the response contract; if it stays "mask", point the
  production docs at `/queryRaw` and add a benchmark assertion that the served
  bytes are the raw mask.

### 2. Stop re-serialising JSON strings that are already JSON (rich endpoints)

- **Effect: medium** on rich requests: `server.mjs:52,74` do
  `JSON.parse(result.toOutcomesJson())` then `res.json(...)`, i.e. a full
  parse + re-stringify of the rich payload per rich request. The rich
  serializer already streams a string (perf-memory 07).
- **Confidence: high** (Express docs: `res.json` stringifies; the value here is
  already a JSON string).
- **Effort: tiny** (send the string with an explicit
  `Content-Type: application/json`; if it must be enveloped, build the envelope
  by string concatenation, not by parsing).
- **Risk: low** (byte-for-byte the same JSON; key order is already pinned by
  `bindings-common` tests).

### 3. Size containers off the cgroup peak, and bound the engine heaps

- **Effect: medium–high** on operational safety (prevents the OOM-kill the
  turf comparison suffered), not on throughput. 128 MB "holds but not
  comfortably" (82–93% of cap, ~9 MiB headroom); target 192–256 MB off
  `memory.peak` (docs:589–597).
- **Confidence: high** for `memory.peak`/`memory.max`/`memory.events`
  semantics (kernel cgroup-v2 docs) and for the measured peak; **medium** for
  `MALLOC_ARENA_MAX` (glibc `malloc(3)` documents per-thread arenas; the exact
  win must be measured).
- **Effort: low** (image/env config). **Risk: low.**
- **First step:** record `memory.peak` at the target request rate and set the
  limit from it; add `--max-old-space-size` (Node) below the limit; A/B
  `MALLOC_ARENA_MAX` under `bench load`.

### 4. Use `queryAsync` deliberately — and correct the pool model first

- **Effect: medium.** Buys event-loop headroom (health checks, co-located work)
  at any load, and throughput on ≥2 workers/cores. Sync remains the right
  default below ~1 ms of query work or on a single core.
- **Confidence: medium.** The direction is solid; the repo has **no** async
  endpoint or harness, and the napi pool model is documented by napi-rs as
  Tokio, not libuv. `UV_THREADPOOL_SIZE` is not the knob for `queryAsync` in
  napi 3.12.1.
- **Effort: medium** (add an async endpoint + extend `bench load`; possibly a
  custom Tokio runtime if the worker count needs raising).
- **Risk: medium** (per-thread prepared memo, in-flight memory multiplier,
  sync rich fallback).
- **First step:** correct the ADR-0009 amendment / ticket-06 wording; then
  A/B sync vs `queryAsync` in `bench load` at the production shape and at
  ~100 candidates to find the crossover.

### 5. `worker_threads` (not `cluster`) to use more cores — when the ruleset is small

- **Effect: high at 30 rules** (up to ~N× the ~165 rps single-thread ceiling);
  **negative at 100k rules** under a cap (100k×10 ruleset = 77.8 MiB/worker).
- **Confidence: high** on the direction and the ruleset numbers; **medium** on
  per-worker isolate overhead (no primary per-worker byte figure).
- **Effort: medium–high** (worker pool, `replace()` broadcast, lifecycle
  handling). **Risk: medium** (duplicated rulesets/memos, Bun worker caveats).
- **Better alternative for large rulesets:** one process + `queryAsync`
  (N compute threads, **one** ruleset; only the thread-local prepared memo
  duplicates). Recommend that over worker_threads once bytes/rule × rules
  becomes significant.
- **First step:** measure per-worker RSS at the production shape; do not
  assume worker_threads scales memory-free.

### 6. Native `filteredGeojson` — only if the endpoint contract changes

- **Effect: low at 1k** (~1–3 ms), **high at 100k** (but 100k is not a
  deployment shape); **conditional** on returning filtered GeoJSON instead of
  a mask.
- **Confidence: medium–high** on the mechanism (serde_json `RawValue` slice +
  concat avoids both DOMs and preserves properties); **low** that it is needed.
- **Effort: medium–high** (new napi method, `raw_value` feature, exact-bytes
  tests). **Risk: medium** (property fidelity if built the wrong way;
  wire-contract addition).
- **Status: already deferred** (ADR-0014; post-v1 "Open proposals"). Treat the
  endpoint's response contract as the trigger.

### 7. Do not bother

- `Buffer.from(mask)`'s 1 KB copy — negligible next to the ~6 ms query.
- Compressing small mask responses — zlib work on the libuv pool for ~1 KB,
  and a `Content-Length` complication.
- Caching the parsed `Query` per request — the query is tiny; the pruning it
  enables is already the win.
- Anything that shares `PreparedGeometry` across threads (ADR-0010) or removes
  the ingestion DOM / `Candidate.id` (perf-memory 08/10) — already decided.

---

## Addendum (2026-09-17): engine-side measurement, and one landed fix

After the study, the engine's own per-request cost was measured directly
(`benchmarks/data/*.geojson`, 1,000 candidates × 30 rules, release, median of 50):

| phase | time |
|---|---|
| candidate ingestion (`candidates_from_geojson`) | 1.09 ms |
| evaluation (`query_mask`) | 3.36 ms |
| total | **4.53 ms** |

Ingestion is ~24% of the engine's per-request work — and the harness dataset
*understates* it: it ships `properties: {}`, while real HTTP payloads carry
metadata the engine **discards** (`Candidate` holds only an id and a geometry).
Parsing that metadata through the `geojson` crate costs more than parsing the
coordinates:

| payload | input | parse before | parse after |
|---|---|---|---|
| `properties: {}` | 296 KiB | 1.091 ms | 1.051 ms |
| 8 properties per candidate | 452 KiB | **2.326 ms** | **1.215 ms** |

**Landed:** `candidates_from_geojson` now uses a candidate-specific deserializer
that reads the top-level `id`, the geometry, and `properties.id` (the fallback)
and streams past every other property without allocating
(`core/src/runtime/ingestion.rs`, perf-memory `.scratch/perf-memory/research/`).
Rules keep the `geojson` path — their properties are queryable. Error codes and
edge behaviour are unchanged (`SR_INVALID_GEOJSON` for malformed JSON, an
unexpected `type`, a missing id, or a missing/malformed geometry), covered by
four added tests in `core/tests/ingestion.rs`.

This was the one remaining measured engine-side reduction; everything in §1–§7
is app-side, already decided, or inherent. The residual candidate-side cost is
the `geojson` crate's `Value` DOM for the *coordinates* (~1.05 ms): parsing
straight into `geo::Geometry` would need a hand-written GeoJSON geometry
deserializer for a ~0.5 ms win, against real edge-matrix risk — left as a
documented option, not done.

---

## Sources

Repo (primary):

- `docs/benchmarks.md` §3, §3b, §HTTP serving memory (`:758–823`, `:543–611`).
- `integration/server.mjs:36,42–57,64–79,84–96`; `integration/memory.mjs`.
- `node/index.ts:162–177,202–312,322–383,391–471`; `node/src/lib.rs:32–161`.
- `core/src/runtime/engine.rs`, `core/src/runtime/evaluate.rs`,
  `core/src/runtime/ingestion.rs`, `core/src/model/candidate.rs`,
  `core/src/runtime/ruleset.rs`.
- `bindings-common/src/lib.rs:27–45,260–282`.
- ADRs 0006, 0009 (+ amendments), 0010, 0014, 0020.
- `.scratch/post-v1/spec.md:25–34`; `.scratch/filtering-scale/issues/03`;
  `.scratch/perf-memory/{spec.md,issues/07,issues/08,issues/10}`;
  `.scratch/architecture-hardening/issues/09`; `.scratch/post-v1/issues/06`.
- `node/Cargo.toml:13`; `Cargo.lock:760` (napi 3.12.1).

External (primary):

- Node.js — "Don't Block the Event Loop":
  https://nodejs.org/en/learn/asynchronous-work/dont-block-the-event-loop
- Node.js — `worker_threads`:
  https://nodejs.org/api/worker_threads.html
- Node.js — C++ addons, "Worker support":
  https://nodejs.org/api/addons.html#worker-support
- Node.js — `http` (`keepAliveTimeout` 5000, `keepAliveTimeoutBuffer` 1000,
  `headersTimeout`, `requestTimeout` 300000, `maxRequestsPerSocket`):
  https://nodejs.org/api/http.html
- Node.js — `Buffer`:
  https://nodejs.org/api/buffer.html
- Node.js — zlib / threadpool usage:
  https://nodejs.org/api/zlib.html#zlib_threadpool_usage
- libuv — thread pool work scheduling (default 4, max 1024, global, shared):
  https://docs.libuv.org/en/v1.x/threadpool.html
- Express — response API (`res.json` = `JSON.stringify`; `res.send` sets
  `Content-Length`):
  https://expressjs.com/en/4x/api/response/
- Express `compression` (zlib/brotli, `threshold` 1 KB, `no-transform`):
  https://github.com/expressjs/compression
- napi-rs — Typed Array (`Buffer` external-buffer zero-copy; `Send + Sync`
  caveat): https://napi.rs/docs/concepts/typed-array
- napi-rs — async fn (Tokio runtime):
  https://napi.rs/docs/concepts/async-fn
- napi-rs — AsyncTask (libuv thread pool):
  https://napi.rs/docs/concepts/async-task
- napi-rs — Async and concurrency guide (decision table; custom Tokio runtime;
  worker shutdown/Bun caveat):
  https://napi.rs/docs/more/async-concurrency
- Linux kernel — Control Group v2, Memory Interface Files (`memory.current`,
  `memory.peak`, `memory.max`, `memory.events`):
  https://docs.kernel.org/admin-guide/cgroup-v2.html
- `malloc(3)` — per-thread arenas (glibc):
  https://man7.org/linux/man-pages/man3/malloc.3.html

No blog posts or Stack Overflow were used. The only claim without a directly
quotable primary figure is the napi-rs default Tokio worker count (not stated
in the docs); it is marked unverified and should be measured.
