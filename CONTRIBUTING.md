# Contributing

Thanks for considering a contribution to `spatial-rules`. This project is a
Rust core with a Node/Bun native addon, and it ships as an npm package.

By contributing you agree to release your work under the project's dual
[MIT](LICENSE-MIT) / [Apache-2.0](LICENSE-APACHE) license.

## Reporting issues

1. Search existing issues first — your problem may already be reported.
2. Open an issue with a clear title and, where possible:
   - a minimal reproduction (GeoJSON rules/candidates + query),
   - the exact error (`SpatialRulesError` code and message),
   - your runtime (Node or Bun, and version).

Issues are triaged with a small label vocabulary. See
[`docs/agents/triage-labels.md`](docs/agents/triage-labels.md) for the
canonical roles (`needs-triage`, `needs-info`, `ready-for-agent`,
`ready-for-human`, `wontfix`). Issue tracking lives in `.scratch/` — see
[`docs/agents/issue-tracker.md`](docs/agents/issue-tracker.md).

## Code of conduct

This project does not maintain a separate code of conduct; treat all
contributors with respect and professionalism.

## Development setup

See [DEVELOPMENT.md](DEVELOPMENT.md) for the full build, test, and benchmark
setup. The short version:

```bash
cargo test --workspace
cargo clippy --workspace --all-targets
cd node && npm install && npm run typecheck
```

## Commit messages

This repository uses **Conventional Commits** — release automation
(`release-please`) derives version bumps and the changelog from commit
messages, so this matters.

Format: `<type>(<scope>): <subject>`

- `type`: `feat`, `fix`, `docs`, `refactor`, `test`, `ci`, `chore`,
  `perf`, `build`.
- `scope`: optional, e.g. `core`, `node`, `docs`.
- `feat` bumps the minor version; `fix` bumps the patch. Everything else
  (including `BREAKING CHANGE:`) is handled by release-please.

Examples:

```
feat(node): add replaceFromCanonical round-trip
fix(core): guard empty ruleset against division by zero
docs: document query shape operators
```

## Pull requests

1. Create a branch from `main`.
2. Make your change; add tests for behavior, not just the happy path.
3. Run the full test suite (see above) and `cargo clippy --all-targets`.
4. Keep commits Conventional-Commits compliant.
5. Open a PR; the CI workflow (`test.yml`) runs Rust tests, clippy, TS
   typecheck, and Node/Bun smoke tests across the OS/Node matrix.

Large, invasive changes should generally be discussed in an issue first.
