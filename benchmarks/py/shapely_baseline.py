"""Shapely 2.x (GEOS) baseline for the Python benchmark (ticket: py-bench).

The engine competitor in the JS harness is turf.js (JSTS). The Python
competitor is Shapely, which wraps GEOS — a native, heavily-optimized C++ DE-9IM
engine with its own prepared-geometry and spatial-index machinery. This is
*why* the Python gap is expected to be far smaller than the turf gap, and why
the benchmark is a real contest rather than a walk-over.

Two rungs, mirroring the engine's B -> F ladder:
  - naive      : per-candidate x per-rule `intersects(c, r)`, with the rule
                 geometries prepared once (GEOS prepared predicate on the rhs).
  - indexed    : a bulk-loaded `STRtree` over the rules + one vectorized
                 `tree.query(cands_array, predicate="intersects")` per batch.

The real "indexed" setup (parse + prepare + STRtree build) is done ONCE outside
the timed region — the same handout the JS harness gives turf (pre-parsed
features, precomputed bboxes). The engine still re-parses its bytes on every
call, so an engine win is conservative (the handicap runs against the engine).

Everything is deterministic and importable without Shapely until the caller
explicitly asks for it, so the harness can report the Shapely version cleanly.
"""

from __future__ import annotations

import json
from typing import Sequence

import numpy as np

__all__ = [
    "shapely_version",
    "load_feature_geometries",
    "build_rule_index",
    "scan_naive",
    "scan_indexed_array",
]


def shapely_version() -> str:
    """Return the Shapely version string, or a clear message if unavailable."""
    try:
        from shapely import __version__
    except ImportError as e:  # pragma: no cover - only hit without shapely
        return f"0.0.0 (shapely not importable: {e})"
    return __version__


def _import():
    """Lazily import the shapely symbols we need.

    A single import guard keeps Shapely's import cost out of the timed regions
    and lets the harness report the version before Shapely is guaranteed.
    """
    from shapely import STRtree, from_geojson, intersects, prepare

    return {"STRtree": STRtree, "from_geojson": from_geojson,
            "intersects": intersects, "prepare": prepare}


def load_feature_geometries(features: Sequence[dict]) -> np.ndarray:
    """Parse a list of GeoJSON feature dicts into a Shapely geometry array.

    Uses the vectorized `from_geojson` entry point (one call for the whole
    batch) — the same "pre-parse outside the timed region" handout the JS
    harness gives turf.
    """
    from_geojson = _import()["from_geojson"]
    return np.asarray(from_geojson([json.dumps(f["geometry"]) for f in features]))


def build_rule_index(rule_geoms: np.ndarray):
    """Prepare the rule geometries and bulk-load a STRtree over them.

    Returns `tree`. The caller keeps it and reuses it across every batch, so
    the one-time build cost lands outside the timed region.
    """
    shapely = _import()
    # `prepare` mutates in place; the STRtree holds references to the same
    # geometry objects, so the prepared forms are shared with the tree.
    shapely["prepare"](rule_geoms)
    return shapely["STRtree"](rule_geoms)


def scan_naive(cand_geoms: np.ndarray, rule_geoms: np.ndarray) -> int:
    """Weakest baseline: every candidate against every rule, exact predicate.

    The `intersects` ufunc automatically uses the prepared rhs geometry, so
    this measures the Python<->GEOS per-pair crossing cost (the naive analog).
    Early-exit on the first match, like the JS harness's `scanMatched`.
    """
    intersects = _import()["intersects"]
    matched = 0
    for c in cand_geoms:
        for r in rule_geoms:
            if intersects(c, r):
                matched += 1
                break
    return matched


def scan_indexed_array(cand_geoms: np.ndarray, tree) -> int:
    """Strongest baseline: one vectorized STRtree query for the whole batch.

    `tree.query(cands, predicate="intersects")` returns a `(2, n)` index array
    where row 0 is the input (candidate) index and row 1 the tree (rule) index
    for every bbox-overlapping, predicate-satisfying pair. A candidate is
    'matched' iff it appears at least once — equivalent to the engine's
    early-exit-on-first-match semantics.
    """
    indices = tree.query(np.asarray(cand_geoms), predicate="intersects")
    if indices.shape[1] == 0:
        return 0
    return int(np.unique(indices[0]).size)
