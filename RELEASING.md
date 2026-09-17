# Releasing

`spatial-rules` (Node/Bun), `spatial-rules-wasm` (npm), and `spatial-rules`
(PyPI) are published from GitHub Actions by `prebuild-publish.yml`, which runs
on a `v*` tag. Releases are cut **manually** with one command:

```bash
bun run release -- 0.3.0            # dry run: prints the plan, writes nothing
bun run release -- 0.3.0 --yes      # execute: bump, changelog, commit, tag, push
```

The three packages share **one version** (lockstep), and the Rust workspace
version tracks it — so a single `vX.Y.Z` tag publishes all three. There is no
release-please; the script (`scripts/release.mjs`) is the release process.

## Making a release

1. Merge the changes you want to ship to `main` (Conventional Commits — the
   changelog is generated from them; see `CONTRIBUTING.md`).
2. From a clean `main`, preview the release:
   `bun run release -- X.Y.Z` — prints the version bump and the generated
   changelog entries for the root, `wasm/`, and `python/`, and writes nothing.
3. Execute it: `bun run release -- X.Y.Z --yes`. The script:
   - bumps every version source to `X.Y.Z`:
     - root `Cargo.toml` `[workspace.package] version` (and `Cargo.lock`, via
       `cargo update --workspace`) — the source maturin reads for the wheel;
     - `node/package.json` version + its 6 `optionalDependencies`;
     - the 6 `node/npm/<platform>/package.json` packages;
     - `wasm/package.json`;
   - prepends the generated changelog entry to `CHANGELOG.md`,
     `wasm/CHANGELOG.md`, and `python/CHANGELOG.md` (git-cliff, `cliff.toml`);
   - commits `chore(release): vX.Y.Z`, tags `vX.Y.Z` (+ the
     `spatial-rules-wasm-vX.Y.Z` / `spatial-rules-python-vX.Y.Z` component tags),
     and pushes.
4. The pushed `vX.Y.Z` tag triggers two workflows: `prebuild-publish.yml`
   builds the 6 platform addons and publishes the 6 platform packages + the
   root `spatial-rules`, `spatial-rules-wasm`, and the PyPI wheel (unchanged
   packages skip idempotently), and `release.yml` creates the **GitHub
   Release** from the matching `CHANGELOG.md` section.

Flags: `--yes` (execute; without it the command is a dry run), `--dry-run`,
`--no-changelog`, `--no-push` (commit + tag locally, push yourself).

## One-time setup (before the first release)

1. Ensure the `NPM_TOKEN` secret exists on the repository (a token with
   `publish` scope, e.g. from an automation account).
2. Ensure the `PYPI_TOKEN` secret exists (a PyPI API token with upload scope for
   the `spatial-rules` project; maturin publishes with username `__token__`).

## Versioning

Follow [SemVer](https://semver.org). Pre-1.0, breaking changes bump the minor
version. The version is chosen by hand — the release script does not infer it
from commit types (git-cliff only groups the changelog).

Every version source the script updates, for reference:

- **root `Cargo.toml`** — `[workspace.package] version`. All workspace crates use
  `version.workspace = true`, so this is the single Rust version; maturin reads
  it for the Python wheel (`python/pyproject.toml` is `dynamic = ["version"]`).
- **node** — `node/package.json` (version + the 6 platform `optionalDependencies`)
  and the 6 `node/npm/<platform>/package.json`, tags `vX.Y.Z`.
- **wasm** — `wasm/package.json`, tag `spatial-rules-wasm-vX.Y.Z`.
- **python** — no file of its own; it inherits the Rust workspace version, tag
  `spatial-rules-python-vX.Y.Z`.

## Manual / emergency

The publish workflow triggers on any `push` of a `v*` tag. To release without the
script (or re-run a publish), tag and push by hand:

```bash
git tag vX.Y.Z <commit>
git push origin vX.Y.Z
```

Note: a tag created through the GitHub API by `GITHUB_TOKEN` (as a bot would)
does **not** trigger other workflows — push it yourself so the event fires.

To create the GitHub Release for a tag that predates `release.yml` (or was
pushed before it existed), run the **release** workflow from the Actions tab
(`Run workflow`, tag input e.g. `v0.2.2`) — it backfills the Release from the
CHANGELOG.

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
