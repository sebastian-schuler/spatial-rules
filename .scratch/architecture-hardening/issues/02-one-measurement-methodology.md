# 02 — One measurement methodology: min-of-N everywhere

Type: task
Status: resolved
Blocked by: None — can start immediately

Origin: 2026-08-19 architecture review, candidate 2.

## What to build

Every benchmark sweep should report the same kind of number. Today the crossover sweep times min-of-N reps while the scale, fair, and complex sweeps time a single shot, yet the benchmark docs claim "both sides are measured min-of-N reps (N = 3)". Make all four sweeps use the same min-of-N timing primitive (it already exists and is shared), and make the documentation describe exactly what the code does — no single-sample numbers presented as damped.

## Acceptance criteria

- [ ] `scale`, `fair`, and `complex` sweeps report min-of-N using the same primitive as `crossover`
- [ ] `docs/benchmarks.md` methodology section matches the code: rep count, damping, and any single-sample statements corrected
- [ ] Sweep output still parses the same way; existing harness consumers (docs, config) unaffected

## Answer

Implemented. `scale`, `fair`, and `complex` now report min-of-`reps` through the
shared `minOf` primitive (same as `crossover`), with a `reps: 3` knob in
`benchmarks.json`. One-time setup costs (ruleset build, prepared-geometry warmup)
stay single-sample and are labelled as such. `docs/benchmarks.md` methodology
matches the code.
