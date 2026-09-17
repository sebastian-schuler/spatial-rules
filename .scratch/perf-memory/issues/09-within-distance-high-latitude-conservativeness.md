# Correctness: `withinDistance` pre-filter is not conservative at high latitude

Type: task
Status: resolved

## What

The `withinDistance` pre-filter can **drop a rule that is genuinely within N
metres**, violating ADR-0016's "never dropping a within-N rule" guarantee. Found
incidentally while working perf-memory 05/07; it is **pre-existing** (reproduced
on `HEAD` before any perf-memory change).

## Reproduction

`core/tests/proptest.rs::within_distance_never_drops_a_within_rule` catches it.
Proptest seed (recorded, then reverted from
`core/tests/proptest.proptest-regressions` while triaging):

```
f7f6b11f0925ab6ce1426ec2a68bae1d9d136c666f29a8a197fe2b082da7f3e9
# shrinks to
#   rects = [RECT(72.30009202046368 71.72880835080687,
#                 80.17047556605961 81.61376390875111)]
#   (lon, lat) = (78.16918899112456, 83.1404328334824)
#   distance = 169682.19460223618
```

For that exact input, on `HEAD` and on the perf-memory branch alike:

- oracle (geo `HaversineClosestPoint` + `Haversine.distance`) = **168 019.6 m**
  (`≤` 169 682.2) → the rule **is** within N;
- engine `query_mask` = **`[0]`** → the rule is dropped.

The seeded test is currently `#[ignore]`d (with a pointer here) so CI stays
green; **un-ignore it when this is fixed**.

## Root cause

The pipeline is *planar pre-filter → spherical confirm*, and the two disagree
about where a rule's edges are.

- The pre-filter (`core/src/runtime/evaluate.rs`, `expand_envelope` +
  `admit_within_distance`) expands the **candidate's** envelope by
  `distance` (lat/lon degrees, latitude-dependent) and queries the R-tree, whose
  entries are the rules' **planar** bounding rectangles.
- The confirm (`min_haversine_distance`) uses geo's `HaversineClosestPoint`,
  which walks rule edges as **great-circle arcs**.

At high latitude a great-circle arc between two vertices at latitude φ bulges
**poleward of the planar envelope**. In the case above the rect's top edge sits
at 81.6138°N but its great-circle arc reaches ≈81.625°N — about 1.2 km of bulge.
The candidate is 1.5267° north of the vertex line, but only ≈1.51° from the
arc, so the true distance is ~1.66 km *less* than the meridional estimate. The
expanded query envelope misses the rule's planar rect by ~0.0006° (~70 m → the
slack), so the rule is never handed to the confirm.

The reverse framing also holds: the engine is internally inconsistent — the
pre-filter assumes planar edges, the confirm assumes spherical ones — and a
pre-filter is only valid if it is conservative with respect to whatever the
confirm computes.

## Fix direction (needs a decision — hence `needs-triage`)

Make the indexed rule extent cover the spherical (great-circle) extent:

1. **Bulge-aware rule envelopes (preferred).** At `Ruleset::build`, store, per
   rule, a spherical envelope that includes each edge's great-circle extremes
   (a closed form exists for the arc's max/min latitude; longitude extremes for
   arcs crossing high latitude likewise). Index that instead of the planar
   envelope. Then the existing candidate-expansion pre-filter is conservative
   again, and the confirm is unchanged.
2. **Carry the bulge into the query.** Expand the candidate's envelope by a
   ruleset-level maximum edge bulge. Weak: a rule edge spanning a large Δλ can
   reach the pole (e.g. 180° of longitude at 80°N), making the bound degenerate
   — so this needs a cap and an argument that real polygons stay under it.
3. **Make the confirm planar** (matching the pre-filter). Rejected on paper:
   it changes ADR-0016's defined semantics (spherical great-circle) and the
   oracle.

Whichever is chosen needs an ADR-0016 amendment and a re-run of the
`withinDistance` ladder (`batch_within_distance`) — option 1 widens the rule
envelopes, which may cost R-tree hits, so the pre-filter's usefulness must be
re-measured.

## Acceptance

- The counterexample above returns `[1]`.
- `within_distance_never_drops_a_within_rule` re-enabled and green across seeds.
- ADR-0016 updated; `batch_within_distance` cost re-measured and recorded.

## Comments

**2026-09-17 (agent): fixed — index carries great-circle envelopes.**

- New `core/src/model/spherical_envelope.rs`: builds a rule's index box by
  sampling each edge's great-circle arc (0.1° steps) and adding an ~11 m margin
  to cover the sampling sagitta (~2.4 m at that step). `Ruleset::build` now
  indexes these instead of `bounding_rect`; the public `Ruleset::envelope`
  accessor and the benchmark `RuleSource` stay planar. Four unit tests pin the
  behaviour (equator ≈ planar, the high-latitude bulge is present and the right
  size, the box contains the arc midpoint, empty → `None`).
- The exact counterexample now returns `[1]`; pinned by
  `core/tests/distance.rs::high_latitude_arc_bulge_does_not_drop_a_within_rule`.
  The proptest is **un-ignored** and ran 30× across seeds with **0 failures**
  (it was failing most runs before). The accidental regression seed my earlier
  runs wrote into `proptest.proptest-regressions` was reverted, and the ticket
  records it — no need to re-add it now that the case is a deterministic
  integration test.
- ADR-0016 amended with the mechanism and the cost.

**Cost (measured, same-session `bun run bench samples` A/B with the planar index
temporarily restored):** `within_distance_mask` **131.9 → 336.9 ms (+156%)**,
`within_distance_full` 131.6 → 336.7 ms, and the point `mask_baseline` 1.50 →
2.74 ms. Correctness wins — the planar pre-filter was silently dropping
genuinely-within rules — but this is a real selectivity regression, so **ticket
11** targets it (per-edge fringe entries). `batch_query` (the main mask ladder)
was unaffected beyond noise.

Gates: `cargo test --workspace --exclude spatial-rules-python` + clippy green.
