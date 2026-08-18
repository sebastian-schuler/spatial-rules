# Spatial Rules Engine — Implementation Specification

**Status:** Draft / Implementation Source of Truth
**Primary target:** Bun / Node.js HTTP applications
**Core implementation:** Rust
**Primary integration:** Node-API / N-API native addon

---

## 1. Purpose

Build a reusable, high-performance Rust spatial rules/query engine for evaluating batches of candidate GeoJSON geometries against a relatively small, dynamically replaceable set of geometry-bearing rules with queryable attributes.

A typical deployment: an HTTP application evaluates batches of candidate
geometries (~1,000 per request) against a small, dynamically replaceable set of
geometry-bearing rules (~30) with queryable properties, in memory-bounded
containers. The library MUST remain generic; any specific domain is an
application concept, not a core-library concept.

The generic abstraction is:

> **A high-performance spatial rule/query engine for evaluating batches of geometries against indexed, attribute-bearing geometry rules.**

---

# 2. Goals

The library MUST:

* Be implemented in Rust.
* Be usable from Node.js.
* Be tested for Bun compatibility.
* Integrate cleanly with HTTP servers such as Express.
* Process candidate geometries in batches.
* Support `Polygon` and `MultiPolygon` initially.
* Store rules as geometry + attributes.
* Support property predicates.
* Support spatial predicates:

  * `intersects`
  * `contains`
  * `within`
* Maintain an indexed, reusable ruleset.
* Support dynamic ruleset replacement without process restart.
* Make replacement safe for concurrent requests.
* Avoid rebuilding indexes for individual users/queries.
* Minimize JavaScript ↔ Rust crossings and unnecessary memory copies.
* Operate within Docker/Kubernetes memory limits.
* Support multiple independent application pods.
* Keep the Rust core independent from Node/Bun.

The architecture SHOULD allow later addition of:

* overlap area
* overlap percentage/ratio
* additional spatial predicates
* richer property expressions
* alternative geometry formats
* Python bindings
* Rust CLI
* other language bindings

---

# 3. Non-Goals

The first version is NOT intended to be:

* a full PostGIS replacement
* a distributed database
* a spatial microservice
* a nearest-neighbor engine
* a routing engine
* a raster/GDAL processing system
* a SQL database
* a domain-specific business rules framework

Nearest-point functionality is explicitly **not a priority**.

The library should be thought of as a **spatial rules/index engine**, rather than literally as Redis.

---

# 4. Core Data Model

## 4.1 Rule

A rule consists of:

* an ID
* properties/attributes
* a geometry

Example:

```json
{
  "id": "rule-17",
  "type": "Feature",
  "properties": {
    "name": "Example Zone",
    "country": "HR",
    "classification": "restricted",
    "priority": 10,
    "active": true
  },
  "geometry": {
    "type": "MultiPolygon",
    "coordinates": []
  }
}
```

The production workload contains approximately 30 rules, but the library MUST NOT impose that as a hard limit.

---

## 4.2 Candidate

A candidate is a geometry being evaluated against the rules.

Example:

```json
{
  "id": "candidate-123",
  "type": "Feature",
  "properties": {},
  "geometry": {
    "type": "Polygon",
    "coordinates": []
  }
}
```

The production workload is approximately 1,000 candidates per query.

---

# 5. Fundamental Query Model

The fundamental operation is a **query/evaluation**, rather than an application-specific "exclude" operation.

Conceptually:

```text
candidate geometry
        +
rule geometry
        +
rule property predicates
        +
spatial predicate
        ↓
      match
```

For example:

```text
Find candidates where there exists an applicable rule such that:

rule.active = true
AND rule.classification = "restricted"
AND intersects(candidate.geometry, rule.geometry)
```

The engine returns matches.

The application decides what those matches mean.

For example:

```text
match → exclude
no match → retain
```

The core MUST NOT encode this interpretation.

---

# 6. Ruleset

A `Ruleset` is an immutable, query-optimized representation of a collection of rules.

Conceptually:

```text
Ruleset
├── rule metadata
├── rule properties
├── geometry storage
├── spatial index
├── property indexes
└── prepared/optimized geometries
```

Once constructed, a ruleset SHOULD be immutable.

This enables:

* concurrent reads
* predictable memory behavior
* safe sharing across requests
* no per-query mutation
* atomic replacement

---

# 7. Dynamic Ruleset Replacement

The rule dataset is dynamic.

Although it may only change approximately once per week, the library MUST support replacement at any time without restarting the application.

The intended lifecycle is:

```text
Current Ruleset
      │
      │ build new ruleset
      ▼
New Ruleset
      │
      ├── parse
      ├── validate
      ├── prepare
      └── build indexes
      │
      ▼
Atomic publication
```

The new ruleset MUST be fully constructed before becoming visible to queries.

The application MUST NOT expose partially built indexes.

Expected behavior:

```text
Ruleset V42
  ├── request A
  └── request B

Build V43

Atomic swap

Ruleset V43
  ├── request C
  └── request D
```

Requests already using V42 may finish against V42.

New requests use V43.

The implementation SHOULD use immutable/shared state and atomic publication semantics.

---

# 8. Multi-Pod Deployment

Each application pod owns its own in-process ruleset.

Example:

```text
Kubernetes

Pod 1
  Bun + Rust engine
  Ruleset V42

Pod 2
  Bun + Rust engine
  Ruleset V42

Pod 3
  Bun + Rust engine
  Ruleset V42
```

When the rules change:

```text
Ruleset V43
  ├── Pod 1 → build/swap
  ├── Pod 2 → build/swap
  └── Pod 3 → build/swap
```

The Rust library MUST NOT implement distributed synchronization.

The application is responsible for distributing updates through mechanisms such as:

* polling
* Redis/pub/sub
* Kafka
* database notifications
* object storage
* configuration services

The Rust library only needs to make ruleset replacement fast and safe.

---

# 9. Rule Exclusions

Rule exclusions are application-level policy.

The Rust core MUST NOT know what a "user" is.

Instead, the application supplies the rules that are applicable to the current
query (via `excludeRuleIds`).

Example:

```text
All rules:

rule-1
rule-2
rule-3
...
rule-30

Excluded rules:

rule-17
rule-21

Applicable rules:

all rules except rule-17 and rule-21
```

The engine MUST NOT rebuild the spatial index for each query.

Because the ruleset is small, an internal bitset/bitmap is a suitable optimization:

```text
Rule IDs:
0 1 2 3 4 5 ... 29

Applicable:
1 1 1 1 0 1 ... 1
```

This is an implementation detail and MUST NOT constrain the public API.

---

# 10. Rule Properties

Rules MUST support queryable properties.

Initial supported value types SHOULD include:

* string
* integer
* floating point
* boolean
* null

Example:

```json
{
  "active": true,
  "country": "HR",
  "priority": 10,
  "classification": "restricted"
}
```

Properties SHOULD be represented internally using compact typed structures rather than retaining arbitrary JavaScript object graphs.

Nested arbitrary objects are not required for the initial implementation.

---

# 11. Property Query Operators

Initial operators:

```text
=
!=
>
>=
<
<=
IN
```

Logical operators:

```text
AND
OR
```

Example conceptual query:

```json
{
  "where": {
    "active": true,
    "classification": "restricted",
    "country": {
      "$in": ["HR", "SI"]
    },
    "priority": {
      "$gte": 5
    }
  }
}
```

SQL itself is out of scope for the first version.

The internal query representation SHOULD be designed so a SQL-like syntax could be added later without changing the underlying engine.

---

# 12. Property Indexing

Property indexes MAY be created during ruleset compilation.

For example:

```text
classification

restricted → [rule IDs]
military   → [rule IDs]
```

and:

```text
active

true  → [rule IDs]
false → [rule IDs]
```

With only ~30 production rules, property indexing is unlikely to be the dominant performance factor.

Nevertheless, the architecture SHOULD support it because:

* it can reduce unnecessary geometry checks
* it provides a clean query planner model
* it allows the library to scale to larger rule sets later

---

# 13. Spatial Predicates

The initial implementation MUST support:

```text
intersects
contains
within
```

Potential future predicates:

```text
covers
covered_by
touches
overlaps
```

Potential quantitative predicates:

```text
overlap_area
overlap_ratio
```

Exact predicate semantics MUST be documented and tested.

---

# 14. Overlap Semantics

The architecture SHOULD eventually support queries such as:

```text
overlap_ratio >= 0.80
```

where:

```text
overlap_ratio =
    area(candidate ∩ rule)
    ----------------------
       area(candidate)
```

It should also support:

```text
overlap_area >= 5 km²
```

These are NOT mandatory for the first production implementation.

When implemented, area calculations MUST have explicitly defined semantics.

WGS84 longitude/latitude coordinates MUST NOT simply be treated as planar X/Y coordinates for meaningful square-kilometre calculations.

---

# 15. Spatial Indexing

The engine MUST avoid performing expensive exact geometry operations against every rule where possible.

The intended evaluation pipeline is:

```text
Candidate geometry
       │
       ▼
Spatial index / bounding-box filtering
       │
       ▼
Possible rules
       │
       ▼
Property filtering
       │
       ▼
Exact geometry predicate
       │
       ▼
Matches
```

The exact spatial index implementation is an internal design decision.

Suitable structures include:

* R-tree
* STRtree
* packed bounding-box tree
* another static spatial index

The selected structure SHOULD be optimized for:

* relatively small static rule sets
* frequent reads
* infrequent rebuilds
* batch candidate queries

---

# 16. Prepared Geometries

Rules are reused across many requests.

Therefore rule geometries SHOULD be prepared/optimized during ruleset construction when the underlying geometry engine supports this.

Lifecycle:

```text
GeoJSON
    ↓
Parse
    ↓
Validate
    ↓
Normalize / prepare
    ↓
Build spatial index
    ↓
Immutable Ruleset
```

This expensive work MUST NOT happen once per HTTP request.

The workload is intentionally asymmetric:

```text
Rules:
  ~30
  rarely change

Candidates:
  ~1,000/request
  constantly change
```

The engine should exploit this by doing expensive work once for the reusable rules.

---

# 17. Query Evaluation Pipeline

For the primary production workload:

```text
~1,000 candidate geometries
            │
            ▼
      spatial index
            │
            ▼
     possible rule matches
            │
            ▼
     property predicates
            │
            ▼
     exact intersection
            │
            ▼
       matched rules
```

The engine SHOULD avoid unnecessary exact geometry operations.

The query planner MAY choose whether property or spatial filtering occurs first based on expected selectivity and cost.

---

# 18. Result Model

The fundamental result SHOULD expose relationships between candidates and rules.

Conceptually:

```text
candidate candidate-123
    matched rules:
      rule-4
      rule-17
```

Possible Rust representation:

```rust
Match {
    candidate_id: CandidateId,
    rule_ids: Vec<RuleId>,
}
```

Future versions may additionally return:

```text
predicate
overlap_ratio
overlap_area
```

For the production filtering path, the Node binding SHOULD support compact results such as:

* candidate indexes
* bitsets
* typed arrays
* rule ID arrays

A diagnostic/richer API can expose candidate-to-rule relationships where required.

---

# 19. Batch API

Batch processing is a mandatory requirement.

The library SHOULD NOT primarily expose an API like:

```javascript
for (const image of images) {
  engine.intersects(image);
}
```

This causes thousands of JavaScript ↔ Rust crossings.

Instead:

```javascript
engine.query(images, query);
```

or:

```javascript
engine.filter(images, query);
```

The intended execution is:

```text
Bun
 │
 │ ONE native call
 │ ~1,000 candidates
 ▼
Rust
 │
 │ all spatial computation
 ▼
Bun
```

Minimizing crossings across the JS/native boundary is a major performance requirement.

---

# 20. Rust Core Architecture

Recommended project structure:

```text
project/
├── core/
│   └── pure Rust spatial engine
│
├── node/
│   └── Node-API binding
│
├── cli/
│   └── optional Rust CLI
│
└── benchmarks/
    └── production-like benchmarks
```

The core MUST remain independent of JavaScript.

It MUST NOT depend on:

* Node.js
* Bun
* Express
* HTTP
* authentication
* user concepts
* domain terminology

Conceptually:

```text
                 spatial-core
                /      |      \
               /       |       \
           Node/Bun   CLI     Python
```

---

# 21. Node-API / N-API

The Node integration SHOULD use Node-API/N-API rather than V8-specific or Node-internal APIs.

This provides a more stable native addon boundary.

Bun compatibility MUST be explicitly tested.

Node-API compatibility MUST NOT be assumed to mean that every addon behaves identically in Bun.

The supported runtime matrix SHOULD initially cover the exact production versions of:

* Node.js
* Bun

Additional versions can be added later.

---

# 22. JavaScript API

An initial conceptual API:

```javascript
import { SpatialRuleset } from "@scope/spatial-rules";

const ruleset = new SpatialRuleset(initialRulesGeoJson);

ruleset.replace(newRulesGeoJson);

const result = ruleset.query(candidateFeatures, {
  spatial: {
    predicate: "intersects"
  },

  where: {
    active: true,
    classification: "restricted"
  },

  excludeRuleIds: ["rule-17", "rule-21"]
});
```

The final API can differ, but the semantics MUST remain.

A filtering-oriented API MAY also exist:

```javascript
const mask = ruleset.filterMask(candidateFeatures, {
  spatial: {
    predicate: "intersects"
  },

  where: {
    active: true
  },

  excludeRuleIds: excludedRuleIds
});
```

---

# 23. GeoJSON Boundary

GeoJSON SHOULD be the main interoperability format because it matches typical external data providers.

However:

> GeoJSON should be treated as an interchange format, not necessarily as the internal representation.

The Rust core SHOULD parse GeoJSON into compact native geometry structures.

The binding SHOULD eventually support Buffer/byte-oriented input to reduce:

* JavaScript object creation
* JSON parsing overhead
* JS ↔ Rust conversion
* memory duplication

Potential API:

```javascript
ruleset.replaceGeoJSON(buffer);

ruleset.queryGeoJSON(candidatesBuffer, query);
```

Object-based APIs MAY also exist for convenience.

---

# 24. Memory Model

The engine has two fundamentally different classes of data.

## Long-lived

```text
~30 rule geometries
spatial index
prepared geometries
rule properties
property indexes
```

This should live in Rust-owned memory.

## Short-lived

```text
~1,000 candidate geometries/request
```

These should be processed and released as soon as practical.

The implementation SHOULD avoid:

```text
JS GeoJSON objects
      +
Rust GeoJSON copies
      +
prepared geometry copies
      +
temporary geometry copies
```

where practical.

The exact memory layout must be benchmarked.

---

# 25. Ruleset Replacement Memory

During replacement, both rulesets may temporarily exist:

```text
Old Ruleset
     +
New Ruleset
     ↓
Atomic swap
     ↓
New Ruleset
```

The old ruleset MUST remain alive while an active query still references it.

Once no active query references it, it SHOULD be released.

Peak memory during replacement MUST be measured because the application runs in constrained containers.

---

# 26. Docker/Kubernetes Requirements

The library MUST operate correctly in memory-limited containers.

The desired deployment is:

```text
┌──────────────────────────────┐
│ Docker container             │
│                              │
│ Bun / Node                   │
│   └── Node-API               │
│        └── Rust engine       │
│             └── Ruleset      │
│                              │
└──────────────────────────────┘
```

The library SHOULD NOT require:

* Rust installation at runtime
* a separate spatial service
* PostGIS
* Redis
* an external database

The initial native binary targets SHOULD prioritize:

```text
linux-x64
linux-arm64
```

The exact libc requirements (`glibc`, `musl`, etc.) MUST be compatible with the supported Docker images.

---

# 27. Package Distribution

The desired developer experience is:

```bash
npm install @scope/spatial-rules
```

Normal users SHOULD NOT need to install Rust.

Prebuilt native binaries SHOULD be distributed for supported platforms.

The exact packaging mechanism can be selected during implementation.

---

# 28. Synchronous vs Asynchronous API

A synchronous API MAY be provided:

```javascript
engine.query(...)
```

if benchmarks show that realistic workloads complete quickly enough not to materially block the event loop.

An asynchronous API SHOULD be available if CPU-intensive workloads can block HTTP handling:

```javascript
await engine.queryAsync(...)
```

The asynchronous implementation should run CPU-intensive Rust work away from the JavaScript main thread.

The decision MUST be benchmark-driven using realistic geometries.

---

# 29. Concurrency

The ruleset is intended to be read concurrently by many HTTP requests.

Requirements:

* concurrent queries MUST be safe
* queries SHOULD NOT require a global mutable lock
* ruleset replacement MUST be safe while queries are active
* active queries may continue using the previous ruleset
* new queries should see the newly published ruleset
* old rulesets must remain alive while referenced

The implementation SHOULD favor immutable shared state.

---

# 30. Performance Strategy

The primary optimization goals are:

1. **Avoid JS ↔ Rust calls per geometry.**
2. **Batch candidate processing.**
3. **Prepare/index rule geometries once.**
4. **Reuse the ruleset across requests.**
5. **Use spatial indexes to reduce exact predicate calls.**
6. **Use property predicates to reduce candidate rules.**
7. **Minimize allocations.**
8. **Minimize geometry copies.**
9. **Return compact results.**
10. **Keep ruleset compilation off the normal request path.**

The important performance insight is:

```text
~30 reusable rules
       versus
~1,000 changing candidates/request
```

The implementation should optimize around this asymmetry rather than assuming both datasets behave similarly.

---

# 31. Benchmark Dataset

Benchmarks MUST include real or representative production-like data:

```text
Rules:
  ~30 polygons/multipolygons
  including simple and highly complex geometries
  including country-scale shapes

Candidates:
  ~1,000 polygons/request
```

Benchmarks SHOULD run enough requests to reveal steady-state behavior.

At minimum:

```text
100 requests
1,000 requests
10,000 requests
```

Measure:

```text
p50 latency
p95 latency
p99 latency
throughput
steady-state memory
peak memory
ruleset build time
ruleset replacement time
```

---

# 32. Algorithm Benchmarks

Compare at minimum:

```text
A. Existing JavaScript implementation

B. Rust naive:
   every candidate × every rule

C. Rust + bounding-box filtering

D. Rust + spatial index

E. Rust + prepared geometries

F. Rust + spatial index + prepared geometries
```

The goal is not merely to demonstrate:

> Rust is faster than JavaScript.

The goal is to determine:

> Which algorithmic optimizations provide the largest benefit for the actual workload?

---

# 33. Correctness Requirements

Correctness is more important than raw performance.

Tests MUST cover:

* valid Polygon
* valid MultiPolygon
* polygons with holes
* touching boundaries
* overlapping boundaries
* identical geometries
* full containment
* disjoint geometries
* very small geometries
* very large geometries
* country-scale geometries
* highly complex geometries
* invalid GeoJSON
* malformed coordinates
* unsupported geometry types
* deterministic results

Spatial results SHOULD be compared against a trusted reference implementation.

---

# 34. Invalid Geometry Handling

The library MUST explicitly define invalid geometry behavior.

Recommended model:

### Rules

Rule geometries SHOULD be validated during ruleset construction.

Invalid rule geometry SHOULD prevent that ruleset from becoming active.

### Candidates

Candidate failures SHOULD preferably be represented at candidate level rather than causing unexplained failure of an entire batch.

A result may distinguish:

```text
matched
not_matched
invalid
```

The exact error model can be finalized during implementation.

---

# 35. Error Model

The library SHOULD distinguish:

```text
invalid GeoJSON
invalid geometry
invalid query
invalid property predicate
ruleset construction failure
unsupported geometry type
unsupported spatial predicate
unsupported property operator
native/runtime error
```

The Node binding SHOULD convert these into normal JavaScript `Error` objects.

Stable error codes SHOULD be provided where practical.

---

# 36. Production Application Flow

The production Bun/Express API should conceptually operate as:

```text
HTTP request
    │
    ▼
authenticate user
    │
    ▼
determine the caller's excluded rules
    │
    ▼
fetch candidate features
    │
    ▼
receive ~1,000 GeoJSON features
    │
    ▼
Rust spatial query
    │
    ├── spatial index
    ├── applicable-rule filtering
    ├── property predicates
    ├── exact geometry predicates
    └── compact result
    │
    ▼
remove excluded candidates
    │
    ▼
HTTP response
```

Rust MUST NOT own:

* authentication
* authorization
* HTTP
* external data-provider calls
* user identity
* domain business semantics

---

# 37. Rule Update Flow

The application obtains new rule data:

```text
Rule source
    │
    ▼
Application
    │
    ▼
ruleset.replace(newRules)
    │
    ├── parse
    ├── validate
    ├── prepare
    ├── build spatial index
    ├── build property indexes
    │
    ▼
atomic publication
```

The HTTP request path SHOULD continue using the old ruleset until the new ruleset is completely ready.

---

# 38. Generic Library Positioning

The library should be positioned as:

> **A high-performance spatial rule/query engine for evaluating batches of geometries against indexed, attribute-bearing geometry rules.**

Potential use cases include:

* exclusion zones
* restricted areas
* flood zones
* delivery/service zones
* aviation restrictions
* protected land
* insurance territories
* geographic content restrictions
* geofencing
* spatial alerting

The library MUST NOT be designed around any specific domain terminology.

---

# 39. Future Extensions

Potential later versions may add:

### Spatial

* `overlaps`
* `touches`
* `covers`
* `covered_by`
* overlap area
* overlap ratio
* distance predicates

### Querying

* richer boolean expressions
* nested predicates
* query planner
* SQL-like query language

### Data

* GeoParquet
* compact binary geometry format
* persisted compiled rulesets

### Language bindings

* Python
* Rust CLI
* additional native bindings

### Scale

* larger rule datasets
* more advanced spatial indexes
* stronger attribute indexing
* spatial aggregations

These MUST NOT expand the first implementation unnecessarily.

---

# 40. Recommended Implementation Order

## Phase 1 — Pure Rust Core

Implement:

```text
Geometry
Rule
Ruleset
SpatialPredicate
PropertyPredicate
Query
Match
```

No Node dependencies.

---

## Phase 2 — Geometry Correctness

Implement and test:

```text
Polygon
MultiPolygon
intersects
contains
within
```

---

## Phase 3 — Ruleset Compilation

Implement:

```text
parse
validate
normalize
prepare
spatial index
property storage
property indexes
```

---

## Phase 4 — Batch Query Engine

Implement:

```text
many candidates
      ↓
spatial filtering
      ↓
property filtering
      ↓
exact predicates
      ↓
matches
```

---

## Phase 5 — Benchmarks

Use realistic rule and candidate datasets.

---

## Phase 6 — Node-API Binding

Expose the minimal high-performance API.

---

## Phase 7 — Bun Integration

Test inside an actual:

```text
Bun + Express + Docker
```

application.

---

## Phase 8 — Dynamic Ruleset Replacement

Implement:

```text
build → validate → index → atomic swap
```

---

## Phase 9 — Memory/Concurrency Testing

Test:

* concurrent requests
* repeated ruleset replacement
* memory limits
* old-ruleset cleanup
* long-running workloads

---

## Phase 10 — Package Distribution

Publish prebuilt native binaries for supported platforms.

---

# 41. Design Principles

## 41.1 Batch first

The primary unit of work is a batch, not an individual geometry.

## 41.2 Prepare once, query many

Rules change infrequently, so expensive preprocessing should be amortized.

## 41.3 Immutable rulesets

Immutable data simplifies concurrency and replacement.

## 41.4 Application policy stays outside

The engine evaluates spatial relationships. The application decides what they mean.

## 41.5 Minimize the JS/native boundary

One batch call is preferable to thousands of native calls.

## 41.6 Minimize copies

Especially for large GeoJSON coordinate arrays.

## 41.7 Optimize actual geometry

Country-scale and highly complex MultiPolygons are important benchmark cases.

## 41.8 Correctness before micro-optimization

A fast incorrect spatial predicate is useless.

## 41.9 No unnecessary service boundary

The default deployment is an embedded native library.

## 41.10 Keep the core reusable

Any specific domain is an application, not the core abstraction.

---

# 42. Open Implementation Decisions

The following decisions remain intentionally open and should be resolved during prototyping:

1. Exact Rust geometry library.
2. Exact spatial index implementation.
3. Exact prepared-geometry implementation.
4. GeoJSON parser.
5. Internal compact geometry representation.
6. Node-API binding implementation.
7. Supported Bun versions.
8. Sync/async API design.
9. Property query AST.
10. Result/bitmask representation.
11. Invalid candidate behavior.
12. Geometry normalization strategy.
13. Ruleset build cancellation/progress.
14. Synchronous/asynchronous ruleset replacement API.
15. Native binary packaging strategy.
16. Whether GeoParquet belongs in core or a separate ingestion package.

These decisions MUST NOT weaken the requirements defined in this specification.

---

# 43. Definition of Done

The first meaningful production release is complete when the following conceptual API works:

```javascript
const ruleset = new SpatialRuleset(rulesGeoJson);

const result = ruleset.query(candidatesGeoJson, {
  spatial: {
    predicate: "intersects"
  },

  where: {
    active: true
  },

  excludeRuleIds: excludedRuleIds
});
```

with:

* approximately 30 complex rule features
* approximately 1,000 candidate features
* dynamic ruleset replacement
* concurrent HTTP requests
* multiple application pods
* bounded container memory
* Bun compatibility
* correct Polygon/MultiPolygon behavior
* no per-feature JS ↔ Rust calls
* measurable performance or resource improvement over the existing implementation

Production readiness requires benchmark evidence and integration testing.

It MUST NOT be considered production-ready solely because the implementation is written in Rust.

---

# 44. Final Architecture

```text
                         Kubernetes
                    ┌──────────────────┐
                    │                  │
                    │  Bun / Express   │
                    │                  │
                    │  ┌────────────┐  │
                    │  │ Node-API   │  │
                    │  │ binding    │  │
                    │  └─────┬──────┘  │
                    │        │         │
                    │  ┌─────▼──────┐  │
                    │  │ Rust Core  │  │
                    │  │            │  │
                    │  │ Ruleset    │  │
                    │  │ Spatial    │  │
                    │  │ Index      │  │
                    │  │ Properties │  │
                    │  │ Predicates │  │
                    │  └────────────┘  │
                    │                  │
                    └──────────────────┘
```

The key data model is:

```text
~30 indexed, attribute-bearing rule geometries
                  │
                  ▼
          immutable ruleset
                  │
                  ▼
       batch-query ~1,000 candidates
                  │
                  ▼
         compact match result
```

The key deployment model is:

```text
one Rust engine per Bun/Node process
one immutable ruleset per process
atomic ruleset replacement
no spatial microservice required
```

The key library abstraction is:

> **Indexed geometry rules + attribute predicates + spatial predicates + batch candidate evaluation.**

An example interpretation is:

> **If a candidate intersects an applicable rule, apply the application-defined outcome (exclude it unless excluded by the query).**

The library itself remains generic.
