// Copies the canonical consumer docs from the repo root into the package dir
// so they ship in the published npm tarball. npm only auto-includes README and
// LICENSE files that live inside the package directory, and only files listed
// in `files` are packed — but the docs are authored at the repo root so that
// they're also the GitHub-facing README/CHANGELOG/LICENSE. This script runs in
// `prepack` to keep the tarball in sync without duplicating sources.
import { copyFileSync, mkdirSync } from 'node:fs';
import { dirname, join } from 'node:path';
import { fileURLToPath } from 'node:url';

const here = dirname(fileURLToPath(import.meta.url));
const pkgDir = join(here, '..');
const repoRoot = join(pkgDir, '..');

const docs = ['README.md', 'LICENSE-MIT', 'LICENSE-APACHE', 'CHANGELOG.md'];

mkdirSync(pkgDir, { recursive: true });
for (const name of docs) {
  copyFileSync(join(repoRoot, name), join(pkgDir, name));
}
