# Changelog

## [0.2.2](https://github.com/sebastian-schuler/spatial-rules/compare/spatial-rules-wasm-v0.2.1...spatial-rules-wasm-v0.2.2) (2026-08-25)


### Bug Fixes

* **wasm:** drop --no-gitignore (unavailable in wasm-pack 0.15), remove pkg/.gitignore so npm packs the wasm binary ([4b325c6](https://github.com/sebastian-schuler/spatial-rules/commit/4b325c6ccaf1868d8a1425f139eaab0de610cd03))
* **wasm:** pack the wasm binary (drop unsupported --no-gitignore) ([2d24017](https://github.com/sebastian-schuler/spatial-rules/commit/2d2401793e1d4abccbc90fe21cfe841e0470034c))

## [0.2.1](https://github.com/sebastian-schuler/spatial-rules/compare/spatial-rules-wasm-v0.2.0...spatial-rules-wasm-v0.2.1) (2026-08-25)


### Bug Fixes

* **release:** auto-publish on release event + wasm/pkg packaging + maturin args ([b1f76bf](https://github.com/sebastian-schuler/spatial-rules/commit/b1f76bf2f29a65cb52146bd6e6906d96548e5a17))
* **release:** drop linked-versions (broken with node tag scheme), bump packages to 0.2.1 ([bea032d](https://github.com/sebastian-schuler/spatial-rules/commit/bea032dc3721bce3db8d287403494c3d76ce4225))
* **release:** drop linked-versions, bump packages to 0.2.1 ([6023158](https://github.com/sebastian-schuler/spatial-rules/commit/60231581d81e6a62e84469c6a3dd13bc8f30224c))
* **release:** wasm pkg in tarball (--no-gitignore), maturin publish args, idempotent npm publish ([08b198a](https://github.com/sebastian-schuler/spatial-rules/commit/08b198a4353c35635205a66fe915190279e04b5e))

## [0.2.0](https://github.com/sebastian-schuler/spatial-rules/compare/spatial-rules-wasm-v0.1.1...spatial-rules-wasm-v0.2.0) (2026-08-25)


### Features

* wasm + Python distribution of the core (spatial-rules-wasm, spatial-rules) ([85f9c01](https://github.com/sebastian-schuler/spatial-rules/commit/85f9c01fa0a4572a69c4b85c2caed209ac889eff))


### Bug Fixes

* **release:** one version across node/wasm/python (linked-versions, 0.1.1) ([7f14265](https://github.com/sebastian-schuler/spatial-rules/commit/7f142655ca722250d4d49cc5b6921da230f5c184))
* **release:** unified versioning + corrected release-please config ([f680282](https://github.com/sebastian-schuler/spatial-rules/commit/f6802823786b81fe795e135e7bf1f23753d0ba5f))
