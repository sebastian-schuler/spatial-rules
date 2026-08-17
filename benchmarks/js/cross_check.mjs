// Cross-checks the Rust predicate results against turf.js (ADR-0008: turf v6,
// JSTS-based, is the trusted reference; any disagreement is investigated, never
// silently accepted).
//
// Usage:
//   1. cargo build --release -p spatial-rules-benchmarks --bin cross_check
//   2. cd benchmarks/js && npm install
//   3. npm run cross-check

import { execFileSync } from 'node:child_process';
import { readFileSync } from 'node:fs';
import { dirname, join } from 'node:path';
import { fileURLToPath } from 'node:url';
import { feature, booleanIntersects, booleanContains, booleanWithin } from '@turf/turf';

const here = dirname(fileURLToPath(import.meta.url));
const pairsFile = join(here, '..', 'data', 'cross_check.json');
const fixtures = JSON.parse(readFileSync(pairsFile, 'utf8'));

const rustBin =
  process.env.CROSS_CHECK_BIN ??
  join(here, '..', '..', 'target', 'release', process.platform === 'win32' ? 'cross_check.exe' : 'cross_check');

const rust = JSON.parse(execFileSync(rustBin, [pairsFile], { encoding: 'utf8' }));

let mismatches = 0;
for (const pair of fixtures.pairs) {
  const a = feature(pair.a);
  const b = feature(pair.b);

  // turf v6 `booleanContains` cannot take a MultiPolygon/GeometryCollection as
  // the contained geometry (feature2) — a known turf limitation (ADR-0008
  // "known quirk"). `intersects` and `within` cover those pairs fully; the
  // skipped `contains` value is hand-verified against DE-9IM.
  const supportsContains = !['MultiPolygon', 'GeometryCollection'].includes(pair.b.type);
  const turf = {
    intersects: booleanIntersects(a, b),
    within: booleanWithin(a, b),
  };
  if (supportsContains) {
    turf.contains = booleanContains(a, b);
  }

  const rustResult = rust.pairs.find((entry) => entry.name === pair.name);
  if (!rustResult) {
    console.error(`missing rust result for ${pair.name}`);
    process.exit(1);
  }
  const predicates = supportsContains
    ? ['intersects', 'contains', 'within']
    : ['intersects', 'within'];
  for (const predicate of predicates) {
    if (rustResult[predicate] !== turf[predicate]) {
      mismatches += 1;
      console.error(
        `MISMATCH ${pair.name}.${predicate}: rust=${rustResult[predicate]} turf=${turf[predicate]}`,
      );
    }
  }
  const containsText = supportsContains ? ` contains=${turf.contains}` : ' contains=skipped(turf-v6-limit)';
  console.log(
    `${pair.name}: intersects=${turf.intersects}${containsText} within=${turf.within}`,
  );
}

if (mismatches > 0) {
  console.error(`${mismatches} mismatch(es)`);
  process.exit(1);
}
console.log('cross-check green');
