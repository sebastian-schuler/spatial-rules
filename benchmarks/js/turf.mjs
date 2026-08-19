// Turf-dependent helpers for the JS harness. Kept separate from `common.mjs`
// (which is deliberately turf-free so the Docker image can import it for
// config without the dev-only turf deps).
//
// The turf baseline escalates across harnesses — naive scan (no index) up to
// bbox fast-reject — but the relate loop itself is one shape; it lives here
// once so every harness measures the same scan.

import { feature, booleanIntersects, bbox } from '@turf/turf';
import { bboxOverlap } from './common.mjs';

// `feature()` objects + precomputed bboxes for a set of GeoJSON features,
// computed once outside the timed region.
export function toTurf(features) {
  const turfFeatures = features.map((f) => feature(f.geometry));
  return { features: turfFeatures, bboxes: turfFeatures.map((f) => bbox(f)) };
}

// "candidate intersects any rule" with early-exit on the first match.
//   - `bbox: false`  → naive scan (the weakest baseline: every rule related)
//   - `bbox: true`   → per-rule bbox fast-reject before the relate (default)
//   - `filter(r)`    → skip rules that fail (e.g. a property `where` clause)
// Returns the matched count; callers assert it equals the addon's mask count.
export function scanMatched(candTurf, ruleTurf, { bbox = true, filter } = {}) {
  let matched = 0;
  for (let c = 0; c < candTurf.features.length; c += 1) {
    const cb = candTurf.bboxes[c];
    for (let r = 0; r < ruleTurf.features.length; r += 1) {
      if (filter && !filter(r)) continue;
      if (bbox && !bboxOverlap(cb, ruleTurf.bboxes[r])) continue;
      if (booleanIntersects(candTurf.features[c], ruleTurf.features[r])) {
        matched += 1;
        break;
      }
    }
  }
  return matched;
}
