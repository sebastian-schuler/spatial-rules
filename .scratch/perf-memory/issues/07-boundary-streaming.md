# Boundary: stream rich JSON, drop the async copy and the unused candidate id

Type: task
Status: resolved

## Goal

Cut transient allocations and a full-buffer copy at the JS↔Rust boundary.

## Plan

- Rich/resolution serializers build a full `serde_json::Value` DOM and then
  stringify it (`bindings-common/src/lib.rs:81,137,185`), with a
  `"literal".to_string()` per key and per outcome. Implement `Serialize` (or
  write into the output `String` directly) to stream without the DOM.
- `queryAsync`/`resolveAsync` copy the entire candidate Buffer
  (`node/src/lib.rs:94,123`). Parse/move candidates BEFORE the async boundary
  (parse on the calling thread, move the `Send` `Vec<Candidate>` into the
  closure) so the raw bytes aren't duplicated.
- `Candidate.id` (`core/src/model/candidate.rs:26`) is allocated and required at
  ingestion but never read by the engine — outcomes are index-aligned. Make it
  optional/lazy or stop requiring it. Breaking public-field change: note the
  version impact.
- `ReplaceReport` is built as a DOM then serialized (`bindings-common/src/lib.rs:44`,
  `node/src/lib.rs:46`) — derive `Serialize` instead.

## Acceptance

- Rich-path allocation count drops measurably (harness or `toOutcomesJson`
  timing); async no longer memcpys the candidate buffer.
- JSON output is byte-identical to today's for every existing smoke/round-trip
  test; wire contract unchanged.
- Any public-type change is recorded (CHANGELOG / ADR if breaking).

## Comments

**2026-09-16 (agent): resolved for the rich-JSON streaming; the rest split to
ticket 10.**

Landed (`bindings-common/src/lib.rs`, plus `serde` added to its deps):

- The rich/resolution serializers no longer build a `serde_json::Value` DOM and
  stringify it. Each outcome is a borrowed view with `#[derive(Serialize)]`, and
  the batch is a hand-written `Serialize` for the runtime-chosen outcome enum —
  hand-written rather than `#[serde(untagged)]` because an untagged enum buffers
  its payload through serde's private `Content`, which is the very DOM this
  exists to avoid. Field declaration order was chosen to match the old
  `serde_json::Map` (a `BTreeMap` here, so alphabetical) **byte for byte**.
- Two exact-string tests pin the wire bytes
  (`query_rich_json_is_byte_identical_to_the_dom_shape`,
  `resolve_rich_json_is_byte_identical_to_the_dom_shape`); all existing
  JSON-value tests are unchanged and green.
- New `batch_rich_json` criterion group in `benchmarks/benches/ladder.rs`
  (adds a `spatial-rules-bindings-common` dev dependency to that crate), so the
  serialization cost is tracked from now on.

**Controlled A/B** (`bun run bench samples --filter=batch_rich_json`, 5 runs
each; the "before" side is `git stash` of just `bindings-common/src/lib.rs`,
which reverts to HEAD's DOM implementation):

| bench | DOM (`BTreeMap` + `Value`) | streaming | delta | noise |
|---|---|---|---|---|
| `query_rich_json` | 167.76 µs | **34.84 µs** | **−79.2%** | ±15.5% |
| `resolve_rich_json` | 471.90 µs | **82.75 µs** | **−82.5%** | ±8.3% |

The DOM was ~5–6× the streaming cost. Absolute numbers are sub-millisecond at
1,000 candidates (rich output is opt-in and separate from the ~15 ms mask path),
so the win matters mainly at large rich batches — but the relative reduction is
large and now measured.

**Deferred to ticket 10** (each needs a design or breaking change, not a trim):
the async candidate-`Buffer` copy, the unused `Candidate.id`, and the
`ReplaceReport` DOM (which is an ADR-0020 interface and costs ~nothing on a
non-per-candidate path).

Everything green: `cargo test --workspace --exclude spatial-rules-python` +
clippy.
