# Boundary remainder: async Buffer copy, unused candidate id, report DOM

Type: task
Status: wontfix

## Why this is separate

Split out of perf-memory 07, whose main item (rich-JSON streaming) landed. Each
of these is either a design change or a breaking change, so they are not
mechanical trims.

## 1. `queryAsync`/`resolveAsync` copy the whole candidate Buffer

`node/src/lib.rs` `query_async`/`resolve_async` do
`candidates.as_ref().to_vec()` before parsing, because the whole `async fn`
body runs on libuv's threadpool and a napi `Buffer` is not `Send`. At 100k
candidates that is a ~10 MB memcpy per call.

To drop it, the candidate parse must happen on the JS thread and the resulting
`Send` `Vec<Candidate>` moved across the async boundary — i.e. restructure from
`#[napi] async fn` to a synchronous method returning a Promise backed by an
async task whose `compute` runs off-thread. That is a change to the ADR-0009
async surface, and it must keep the same `Promise<QueryResult>` /
`Promise<Uint8Array>` contract and the same `SR_*` rejection behaviour. Confirm
napi-rs's execution model for `async fn` before designing.

## 2. `Candidate.id` is stored but never read

`core/src/model/candidate.rs` keeps `id: String`, ingestion *requires* one
(`extract_feature_id`), and nothing in `core`/`bindings-common` ever reads it —
outcomes are index-aligned. At 100k candidates that is 100k needless `String`
allocations plus storage. Make it optional/lazy, or stop requiring it. This is a
**breaking public-field change** — record it (CHANGELOG / semver bump).

## 3. `ReplaceReport` is built as a DOM then serialized

`bindings-common::report_to_json` returns a `serde_json::Value` that node/python
then stringify (`serde_json::to_string(&report_to_json(..))`). Four fields, called
only on `replace`/`health`/`stats` — negligible cost. Left alone deliberately:
`report_to_json` is an interface ADR-0020 names. If it is changed, prefer
deriving `Serialize` on `ReplaceReport` (camelCase) and amending ADR-0020, and
accept the key order changing from the current alphabetical `BTreeMap` order
(consumers `JSON.parse`, so this is cosmetic).

## Acceptance

- Async no longer copies the candidate buffer; smoke + `--experimental-strip-types`
  tests green under Node 22/24/26 and Bun.
- Candidate `id` change recorded as breaking; outcomes still index-aligned.
- Any interface change reflected in ADR-0020.

## Comments

**2026-09-17 (agent): wontfix — all three decided against.**

1. **Async `Buffer` copy — keep it.** The copy exists because `#[napi] async fn`
   bodies run off the JS thread — a NAPI-RS-managed **Tokio** runtime in napi-rs
   3.x, *not* libuv (corrected 2026-09-17; see ADR-0009) — while a napi
   `Buffer`'s bytes cannot be safely shared across threads (napi-rs documents
   `Buffer` as `Send + Sync` but warns the bytes are not synchronized). And the
   *parse* is the expensive part at large batches (100k candidates ≈ 456 ms,
   parse-dominated). Removing the copy means parsing on the JS thread before the
   async boundary, which puts the dominant cost back on the event loop —
   defeating ADR-0009's reason for having an async path at all. The copy is the
   deliberate price of keeping the parse off-thread; no change.
2. **`Candidate.id` — keep it.** The engine never reads it (outcomes are
   index-aligned), but ingestion *requires* an id by contract (the
   `edge_matrix`/`ingestion` tests pin that a feature without one is an error),
   so this is not a mechanical trim: allowing id-less candidates is a
   behavioural change, and dropping the field is a breaking public-API change.
   The win is one `String` per candidate — ~1% on the 100k-candidate parse the
   subagent flagged as a hotspot. Not worth the contract change.
3. **`ReplaceReport` DOM — leave it.** Four fields, called only on
   `replace`/`health`/`stats`, and `report_to_json` is an interface ADR-0020
   names. Changing it would touch core + node + python + the ADR for a
   non-measurable gain.

If any of these is revisited, 1 needs a zero-copy way to hand JS-owned memory to
another thread (none exists today), and 2 needs a deliberate decision to let
candidates go id-less.
