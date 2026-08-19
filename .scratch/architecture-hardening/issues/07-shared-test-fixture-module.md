# 07 — Shared test-fixture module for the core suite

Type: task
Status: resolved
Blocked by: None — can start immediately

Origin: 2026-08-19 architecture review, candidate 7.

## What to build

The core test suite should define its fixtures once. Today the same unit-square polygon builder is copy-pasted across several integration test files, along with rule/candidate builders and a jittered shape that duplicates the benchmark dataset's generator — each copy a chance to drift (winding, holes, bounds). Create a shared fixture module the whole suite consumes, and remove the per-file duplicates. The suite's breadth stays exactly as wide; only the duplication goes away.

## Acceptance criteria

- [ ] The unit-square polygon, rule/candidate builders, and jittered shape are defined once and shared by all core integration tests
- [ ] No fixture builder is copy-pasted across test files; per-file duplicates removed
- [ ] Full core test suite green with no behavior changes
- [ ] `docs/test-matrix.md` updated if the fixture move shifts any file→coverage-owner mapping

## Answer

Implemented. `core/tests/common/mod.rs` defines the unit-square polygon,
rule/candidate builders, and the jittered ring once; all core integration test
files consume it and the per-file duplicates were removed. The
file→coverage-owner mapping is unchanged (noted in `docs/test-matrix.md`).
Full core suite green.
