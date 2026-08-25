# Changelog

## [0.2.1](https://github.com/sebastian-schuler/spatial-rules/compare/v0.2.0...v0.2.1) (2026-08-25)


### Bug Fixes

* **release:** drop linked-versions (broken with node tag scheme), bump packages to 0.2.1 ([bea032d](https://github.com/sebastian-schuler/spatial-rules/commit/bea032dc3721bce3db8d287403494c3d76ce4225))
* **release:** drop linked-versions, bump packages to 0.2.1 ([6023158](https://github.com/sebastian-schuler/spatial-rules/commit/60231581d81e6a62e84469c6a3dd13bc8f30224c))

## [0.2.0](https://github.com/sebastian-schuler/spatial-rules/compare/v0.1.1...v0.2.0) (2026-08-25)


### ⚠ BREAKING CHANGES

* **node:** queryAsync returns Promise<QueryResult> instead of Promise<Uint8Array>; queryOutcomes() is removed.
* **node:** toMask, toIndices, toRichJson, queryRich, and toJSON are renamed on the public API.

### Features

* **core:** canonical ruleset persistence (ticket 04) ([a470ce2](https://github.com/sebastian-schuler/spatial-rules/commit/a470ce2afb3f5b39b3e8d1e9f8aed0027a41be04))
* **core:** per-candidate aggregation over the applicable rule set (ADR-0018, tickets 01-03) ([f7e4517](https://github.com/sebastian-schuler/spatial-rules/commit/f7e451786c792e14503691dc943435f6642cd4ed))
* **core:** point candidates and whole-clause $nor ([41b93f3](https://github.com/sebastian-schuler/spatial-rules/commit/41b93f330d9f08bde83c2c7a52a0daf2d9cd7086))
* **core:** quantitative overlap area/ratio on the rich path (ticket 03) ([cf4b9fa](https://github.com/sebastian-schuler/spatial-rules/commit/cf4b9fadee38b3e2cc02514d1dce7b4f10363be5))
* **core:** richer where operators $not/$nin/$exists (ticket 01) ([371ad44](https://github.com/sebastian-schuler/spatial-rules/commit/371ad4492a8b89abf1e27c2556fc33932ba98857))
* **core:** spatial predicates covers/covered_by/touches/overlaps (ticket 02) ([f683a5c](https://github.com/sebastian-schuler/spatial-rules/commit/f683a5cfd243b471d4c8df665ff913aaed61512d))
* **core:** temporal conditions and withinDistance (P2 realistic rules, tickets 02+03) ([bdf11ea](https://github.com/sebastian-schuler/spatial-rules/commit/bdf11eab9a84833cde67c5218b18a1f9354926bd))
* **node:** chainable query result (ticket 03) ([edc0df7](https://github.com/sebastian-schuler/spatial-rules/commit/edc0df78dcaca5e6333681955f3a7cf298047b94))
* **node:** dynamic input types (ticket 05) ([0cbf720](https://github.com/sebastian-schuler/spatial-rules/commit/0cbf720655d6faf8d127434a3a723fce6836a07b))
* **node:** memory-lean query result terminals ([79b3366](https://github.com/sebastian-schuler/spatial-rules/commit/79b3366f43b7e6868c5b52a1b49af493c45a17c8))
* **node:** migrate wrapper to TypeScript, compile-on-publish (ts-migration 02) ([b59e414](https://github.com/sebastian-schuler/spatial-rules/commit/b59e4146483a5a6e98a7ccbb6f1c577263f8631b))
* **node:** opt-in off-main-thread queryAsync (ticket 06) ([5ce241d](https://github.com/sebastian-schuler/spatial-rules/commit/5ce241d004ff7d4c61c40b2f596056ad2c6a7b4c))
* **node:** resolve()/resolveAsync() — ResolutionResult mask, count, summary, lazy toJson ([75fc5f5](https://github.com/sebastian-schuler/spatial-rules/commit/75fc5f5055b6cda6f71fabac398a3b7ebb9daa79))
* **node:** TS tooling bootstrap — tsconfig, native.d.ts, typecheck (ts-migration 01) ([eb8f1b6](https://github.com/sebastian-schuler/spatial-rules/commit/eb8f1b636e4944056ac3cf6ca4600e24d93d7e15))


### Bug Fixes

* **ci:** load local addon in smoke, exclude python crate from rust tests, pyo3 extension-module ([3d3f907](https://github.com/sebastian-schuler/spatial-rules/commit/3d3f9075bd94ded592453a6ab70f5c10590c39fe))
* **core:** reject negative priority at both ingestion gates; resolve P1 tickets ([871575a](https://github.com/sebastian-schuler/spatial-rules/commit/871575a8a67baa5d5c1119ce8641bdbd7f61f3d8))
* **release:** bump to 0.1.1 ([0cf31fb](https://github.com/sebastian-schuler/spatial-rules/commit/0cf31fbefe604f9c2939f8355408c6c16c76f474))
* **release:** drop prepublishOnly napi prepublish; make publish idempotent ([f68f004](https://github.com/sebastian-schuler/spatial-rules/commit/f68f0040b56d1d20e564f682f75405db5961afbb))


### Code Refactoring

* **node:** rename exported API for clarity ([b7c3eef](https://github.com/sebastian-schuler/spatial-rules/commit/b7c3eefde3fe93b10fb5e7bd89b07485a79ff795))
* **node:** unify query/queryAsync behind QueryResult ([431d2b5](https://github.com/sebastian-schuler/spatial-rules/commit/431d2b536384789740adb8fb72ea1cb26c2d5fb7))

## [0.1.0](https://github.com/sebastian-schuler/spatial-rules/compare/spatial-rules-v0.1.0...spatial-rules-v0.1.0) (2026-08-20)


### Features

* **core:** canonical ruleset persistence (ticket 04) ([a470ce2](https://github.com/sebastian-schuler/spatial-rules/commit/a470ce2afb3f5b39b3e8d1e9f8aed0027a41be04))
* **core:** point candidates and whole-clause $nor ([41b93f3](https://github.com/sebastian-schuler/spatial-rules/commit/41b93f330d9f08bde83c2c7a52a0daf2d9cd7086))
* **core:** quantitative overlap area/ratio on the rich path (ticket 03) ([cf4b9fa](https://github.com/sebastian-schuler/spatial-rules/commit/cf4b9fadee38b3e2cc02514d1dce7b4f10363be5))
* **core:** richer where operators $not/$nin/$exists (ticket 01) ([371ad44](https://github.com/sebastian-schuler/spatial-rules/commit/371ad4492a8b89abf1e27c2556fc33932ba98857))
* **core:** spatial predicates covers/covered_by/touches/overlaps (ticket 02) ([f683a5c](https://github.com/sebastian-schuler/spatial-rules/commit/f683a5cfd243b471d4c8df665ff913aaed61512d))
* **node:** chainable query result (ticket 03) ([edc0df7](https://github.com/sebastian-schuler/spatial-rules/commit/edc0df78dcaca5e6333681955f3a7cf298047b94))
* **node:** dynamic input types (ticket 05) ([0cbf720](https://github.com/sebastian-schuler/spatial-rules/commit/0cbf720655d6faf8d127434a3a723fce6836a07b))
* **node:** memory-lean query result terminals ([79b3366](https://github.com/sebastian-schuler/spatial-rules/commit/79b3366f43b7e6868c5b52a1b49af493c45a17c8))
* **node:** migrate wrapper to TypeScript, compile-on-publish (ts-migration 02) ([b59e414](https://github.com/sebastian-schuler/spatial-rules/commit/b59e4146483a5a6e98a7ccbb6f1c577263f8631b))
* **node:** opt-in off-main-thread queryAsync (ticket 06) ([5ce241d](https://github.com/sebastian-schuler/spatial-rules/commit/5ce241d004ff7d4c61c40b2f596056ad2c6a7b4c))
* **node:** TS tooling bootstrap — tsconfig, native.d.ts, typecheck (ts-migration 01) ([eb8f1b6](https://github.com/sebastian-schuler/spatial-rules/commit/eb8f1b636e4944056ac3cf6ca4600e24d93d7e15))

## Changelog

All notable changes to this project will be documented in this file.

The format is based on [Keep a Changelog](https://keepachangelog.com/en/1.1.0/),
and this project adheres to [Semantic Versioning](https://semver.org/spec/v2.0.0.html).

This file is generated and maintained by
[release-please](https://github.com/googleapis/release-please) from
[Conventional Commits](https://www.conventionalcommits.org/) — do not edit it by
hand.
