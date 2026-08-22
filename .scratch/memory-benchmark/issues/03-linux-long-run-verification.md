# 03 — Linux long-run memory verification and re-record

Type: task
Status: ready-for-agent
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

- [ ] `bun run bench memory-scale` full default grid on Linux: the `bounded`
      verdicts re-checked — the Windows `bounded: false` cells are expected to
      report `bounded: true` on Linux glibc allocators (no arena warmup); a
      50+ swap probe at 100k×100 confirms a flat plateau (no per-replacement
      leak), closing the "no leak claim either way" caveat from ticket 01
- [ ] Serving-footprint numbers re-recorded on Linux where they differ from
      the Windows table (RSS is platform-sensitive; the *ratios* — ruleset vs
      serving vs turf — should hold)
- [ ] Cold-batch latency and warm `queries_per_sec` re-recorded on Linux; the
      "throughput unchanged" claim (ticket 02) re-confirmed there
- [ ] `bun run bench memory-turf` turf comparison re-recorded on Linux
- [ ] Results recorded in `docs/benchmarks.md` §Memory with a platform note
      (the table gains a "platform" column or a per-platform breakdown); the
      container baseline (architecture-hardening 09, ~65 MB peak vs 128 MB
      bound, inside the pinned Docker image) re-confirmed on Linux
- [ ] Ticket 01's caveats ("the big cells' plateau is unobserved within 20
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