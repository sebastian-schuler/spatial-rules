# Releasing

`spatial-rules` (Node/Bun), `spatial-rules-wasm` (npm), and `spatial-rules`
(PyPI) are published from GitHub Actions. The release pipeline is largely
automated with [release-please](https://github.com/googleapis/release-please).

## How it works

```
commit on main (Conventional Commits)
   └─ test.yml          (gate: Rust tests, clippy, TS typecheck, Node/Bun/wasm/deno/python smoke)
        └─ release-please (on main) opens a Release PR:
             - bumps versions in node/package.json, wasm/package.json, python/
             - keeps node/package-lock.json in sync (commit it first)
             - generates CHANGELOG.md at the repo root, wasm/CHANGELOG.md, python/CHANGELOG.md
             - bumps the 6 platform packages in lockstep via extra-files (version-locked)
        └─ merge the Release PR
             └─ release-please creates a vX.Y.Z tag + GitHub Release
                  └─ prebuild-publish.yml (on v* tags)
                       - builds the 6 platform addons (cargo/cross)
                       - publishes the 6 platform packages
                       - publishes the root package
                       - publishes spatial-rules-wasm to npm
                       - publishes spatial-rules to PyPI
```

The npm version and the Rust workspace version are **independent** — release-please
only manages the packages. Nothing is published until you merge the
Release PR, so releases are deliberate.

## One-time setup (before the first release)

1. Ensure the `NPM_TOKEN` secret exists on the repository (a token with
   `publish` scope, e.g. from an automation account).
2. Ensure the `PYPI_TOKEN` secret exists (a PyPI API token with upload scope for
   the `spatial-rules` project; maturin publishes with username `__token__`).
3. Confirm `release-please.yml`, `release-please-config.json`, and
   `.release-please-manifest.json` are present and committed.

## Making a release

1. Merge the changes you want to ship to `main` (Conventional Commits).
2. release-please opens a **Release PR** (e.g. "chore(main): release
   spatial-rules 0.2.0"). Review the version bump and the generated
   `CHANGELOG.md` in that PR.
3. Merge the Release PR. release-please tags `v0.2.0` and creates a GitHub
   Release; `prebuild-publish.yml` builds and publishes all packages.

## Manual/emergency releases

The `prebuild-publish.yml` workflow also supports `workflow_dispatch` for the
build matrix, but publishing is restricted to `v*` tags. To force a publish
without release-please, push a `vX.Y.Z` tag to the matching commit.

## Versioning

Follow [SemVer](https://semver.org). `feat` → minor, `fix` → patch;
`BREAKING CHANGE:` in the commit footer → major. Pre-1.0, breaking changes bump
the minor version (release-please config sets `bump-minor-pre-major: true` for
all three packages).

The npm/PyPI versions and the Rust workspace version are **independent** —
release-please manages the packages. Per-package version sources:

- **node** — `node/package.json` (+ the 6 platform packages in lockstep via
  `extra-files`), tags `vX.Y.Z`.
- **wasm** — `wasm/package.json`, tags `spatial-rules-wasm-vX.Y.Z`.
- **python** — `pyproject.toml` declares `dynamic = ["version"]` and maturin
  reads the version from the Rust workspace (`[workspace.package] version` in
  the root `Cargo.toml`); release-please updates that workspace version via
  `extra-files` (toml, `$.workspace.package.version`). Tags
  `spatial-rules-python-vX.Y.Z` (a distinct component — the plain
  `spatial-rules-*` tag namespace predates node's switch to `v*`).

## Verification

After publishing, verify with a clean install in a throwaway directory:

```bash
npm init -y
npm pkg set type=module
npm install spatial-rules
node -e "import('spatial-rules').then(m => console.log(Object.keys(m)))"
```

See the `clean-install` job in `.github/workflows/test.yml` for the CI version
of this check.
