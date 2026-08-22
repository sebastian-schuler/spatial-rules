# 03 — Linux long-run memory verification and re-record

Type: task
Status: resolved
Blocked by: None — can start immediately

Origin: follow-up to memory-benchmark tickets 01 + 02. The memory numbers
recorded in `docs/benchmarks.md` §Memory were measured on Windows; the
lifecycle trace for the big cells (100k×100) only ran 20 swaps, and several
cells report `bounded: false` (allocator-arena warmup, not a leak). Tickets
01/02 documented the Linux run as the outstanding proof.

## Question / what to build

Re-verify the memory and lifecycle claims on Linux (the platform a container
runs on) and re-record any numbers that differ. No behavior change —
measurement + docs only.

## Acceptance criteria

- [x] `bun run bench memory-scale` full default grid on Linux: the `bounded`
      verdicts re-checked — the Windows `bounded: false` cells are expected to
      report `bounded: true` on Linux glibc allocators (no arena warmup); a
      50+ swap probe at 100k×100 confirms a flat plateau (no per-replacement
      leak), closing the "no leak claim either way" caveat from ticket 01
- [x] Serving-footprint numbers re-recorded on Linux where they differ from
      the Windows table (RSS is platform-sensitive; the *ratios* — ruleset vs
      serving vs turf — should hold)
- [x] Cold-batch latency and warm `queries_per_sec` re-recorded on Linux; the
      "throughput unchanged" claim (ticket 02) re-confirmed there
- [x] `bun run bench memory-turf` turf comparison re-recorded on Linux
- [x] Results recorded in `docs/benchmarks.md` §Memory with a platform note
      (the table gains a "platform" column or a per-platform breakdown); the
      container baseline (architecture-hardening 09, ~65 MB peak vs 128 MB
      bound, inside the pinned Docker image) re-confirmed on Linux
- [x] Ticket 01's caveats ("the big cells' plateau is unobserved within 20
      swaps", "Windows-only") removed or updated to the Linux-verified state

## Notes

- The production-relevant question this closes: "does a long-lived serving
  process leak across ruleset replacements at 100k rules?" The 1k×1k 50-swap
  probe already plateaus flat on Windows; the 100k×100 long run is the gap.
- Runs in the pinned Docker image (oven/bun:1.3.14, architecture-hardening
  06) so the numbers match what a deployed container sees.
- Orthogonal deferral tracked separately: the geo 0.34 `Rc → Arc` per-thread
  duplication fix (`.scratch/post-v1/issues/05-geo-034-upgrade.md`) is not part
  of this ticket.

## Answer

Resolved. Re-verified the whole memory picture on Linux (the deploy platform)
inside the pinned container, plus two measurement bugs surfaced and fixed.

- **Reproducible harness.** New `benchmarks/Dockerfile` builds the release
  `memory_scaling` binary + node addon for linux-gnu and runs under
  `oven/bun:1.3.14`; `docs/benchmarks.md` §Memory documents the four run
  commands. Fixes: `benchmarks/src/rss.rs` — the `/proc/self/status` parser
  didn't skip the `:` after the field name, so every in-container RSS reading
  silently read 0 (the Linux numbers were never measured before); a regression
  test pins the format. `.dockerignore` — `*.node` didn't match nested addons,
  so a stale Windows `spatial_rules.node` shadowed the Linux one; `**/*.node`.
- **Full grid (Linux).** Ruleset/serving/qps within ~2–5% of Windows (Linux a
  bit leaner/faster); serving at 100k×100 = **282 MiB** (vs 1.78 GiB eager);
  cold first batch 7 ms (vs 1.9 s eager); warm qps unchanged. The `bounded`
  verdict is a quarter-mean *heuristic*: 3 cells report `false` at 20 swaps,
  and the traces explain each as either a one-time warmup plateau or a
  **sawtooth up-slope** — not a leak.
- **50-swap probes settle the leak question.** 100k×100 oscillates between
  ~561 and ~774 MiB over 50 swaps, 100k×10 between ~286 and ~543 MiB — glibc
  grows the arena ~21 MiB per swap and trims it back every ~11–18 swaps,
  returning to the **same floor** each time (commit tracks RSS; a leak climbs
  monotonically). Replacement peak is the capacity number: 100k×100 peaks
  ~492 MiB above its floor (~774 MiB resident) while holding a ~258 MiB
  ruleset.
- **Turf + container baseline re-recorded.** Engine serving beats turf at every
  cell except 100k×10 (within ~8%); 100k×100 serving 279 MiB vs turf 634 MiB.
  Production 30-rule workload: peak resident **~67 MB** (was ~65 MB) against
  the 128 MB bound; replacement spread ≈ −1 MB (no leak).
- **Docs:** `docs/benchmarks.md` §Memory now has Windows + Linux tables (with
  platform notes), the definitive sawtooth/no-leak finding, the reproducible
  Linux method, and updated container/load baselines; README + roadmap P0
  refreshed; ticket 01's caveats updated. Full workspace suite green on Linux
  (the rss tests that CI had been failing on now pass).