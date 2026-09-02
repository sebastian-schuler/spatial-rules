# Python benchmark: engine PyO3 wheel vs Shapely/GEOS — core `intersects`.
#
#   python benchmarks/py/bench.py [--reps=3] [--points=30,300,1000]
#                                 [--rules=30] [--candidates=1000]
#                                 [--rules-file=benchmarks/data/rules.geojson]
#
# Mirrors `bun run bench perf` (server-bench.mjs) on the engine side and the
# JS `sweeps.mjs` generators on the workload side, so the two halves of the
# comparison follow the same fairness conventions:
#   - The engine is timed for the full steady-state call a user makes:
#     `Ruleset.query(bytes, query)` — GeoJSON parse + PyO3 + index + relate +
#     mask, every query. This is the same "addon carries its parse + FFI cost"
#     handicap the JS harness applies to the native addon.
#   - The Shapely side is handed pre-parsed geometries, a prebuilt STRtree and
#     prepared rule forms — all one-time setup excluded from timing — and is
#     measured for the relate/scan only. So an engine win is conservative.
#   - Correctness: both sides must report the same matched count *before*
#     timing (min-of-N is only run on agreeing implementations).
#   - Every timed number is min-of-`reps`, per the shared methodology.
#
# All knobs default to the `python` section of benchmarks.json; flags override.
# Invoked from the repo root via `bun run bench python`.
#
# Real-data mode (`--rules-file`) evaluates the committed dataset only: the
# "rules sweep" needs the deterministic synthetic grid generator, which has no
# meaning against a single arbitrary boundary file, so it is skipped.

from __future__ import annotations

import argparse
import json
import math
import time
from pathlib import Path
from typing import List, Sequence

REPO_ROOT = Path(__file__).resolve().parents[2]

# The one spatial-only query every harness shares (shared/config.mjs
# `SPATIAL_QUERY`) — defined here so the engine side can't drift.
SPATIAL_QUERY = {"spatial": {"predicate": "intersects"}}


# ---- config ----------------------------------------------------------------

def load_config() -> dict:
    return json.loads((REPO_ROOT / "benchmarks.json").read_text())


# ---- workload generators (ports of benchmarks/js/common.mjs) ---------------

def make_rng(seed: int):
    """Deterministic 32-bit LCG — identical to the JS `makeRng`."""
    state = seed & 0xFFFFFFFF

    def rng() -> float:
        nonlocal state
        state = (state * 1664525 + 1013904223) & 0xFFFFFFFF
        return state / 4294967296

    return rng


def blob_ring(rng, cx: float, cy: float, radius: float, vertices: int) -> list:
    """A closed, star-shaped ring (always valid), matching `blobRing`."""
    coords = []
    for i in range(vertices):
        angle = (i / vertices) * math.tau
        r = radius * (0.7 + 0.5 * rng())
        coords.append([cx + r * math.cos(angle), cy + r * math.sin(angle)])
    coords.append(coords[0])
    return coords


def make_rule_features(n: int) -> List[dict]:
    """`n` complex blob rules on a grid (see `makeRules` in common.mjs)."""
    side = int(math.ceil(math.sqrt(n)))
    rng = make_rng(0x5EE00000 ^ n)
    features = []
    for i in range(n):
        col = i % side
        row = i // side
        cx = col + 0.5
        cy = row + 0.5
        radius = 0.35
        exterior = blob_ring(rng, cx, cy, radius, 120 + int(rng() * 180))
        holes = [blob_ring(rng, cx, cy, radius * 0.4, 24)] if rng() < 0.35 else []
        features.append({
            "type": "Feature",
            "id": f"rule-{i}",
            "properties": {"classification": f"class-{i % 5}"},
            "geometry": {"type": "Polygon", "coordinates": [exterior, *holes]},
        })
    return features


def make_grid_candidates(m: int, rule_count: int, rng) -> List[dict]:
    """`m` small square candidates centred on cell centres (see makeGridCandidates)."""
    side = int(math.ceil(math.sqrt(rule_count)))
    features = []
    for i in range(m):
        cx = math.floor(rng() * side) + 0.5
        cy = math.floor(rng() * side) + 0.5
        w = 0.05
        features.append({
            "type": "Feature",
            "id": f"cand-{i}",
            "properties": {},
            "geometry": {
                "type": "Polygon",
                "coordinates": [
                    [[cx - w, cy - w], [cx - w, cy + w], [cx + w, cy + w], [cx + w, cy - w], [cx - w, cy - w]]
                ],
            },
        })
    return features


def to_collection(features: Sequence[dict]) -> dict:
    return {"type": "FeatureCollection", "features": list(features)}


# ---- engine + timing helpers ----------------------------------------------

def engine_matched(ruleset, candidates, query: str) -> int:
    mask = ruleset.query(candidates, query)
    return sum(1 for v in mask if v == 1)


def timed(fn) -> float:
    start = time.perf_counter()
    fn()
    return (time.perf_counter() - start) * 1000


def min_of(fn, reps: int) -> float:
    best = float("inf")
    for _ in range(reps):
        ms = timed(fn)
        if ms < best:
            best = ms
    return best


def speed_label(speedup: float) -> str:
    return f"{speedup:.0f}x" if speedup >= 100 else f"{speedup:.1f}x"


def vs_engine_cell(baseline_ms: float, engine_ms: float) -> str:
    """How the engine compares to a baseline, with the winner made explicit.

    Returns a short cell like `engine 2.5x` (engine is faster) or
    `Shapely 4.3x` (the baseline is faster).
    """
    ratio = baseline_ms / engine_ms  # engine_ms / baseline_ms → engine speed
    if ratio >= 1:
        return f"engine {speed_label(ratio)}"
    return f"Shapely {speed_label(engine_ms / baseline_ms)}"


def table_row(name: str, ms: float, engine_ms: float) -> str:
    return f"{name:<28} {ms:>10.2f} ms   {vs_engine_cell(ms, engine_ms):>14}"

# ---- reference workload (committed 30x1000 set) ----------------------------

def run_reference(cfg: dict, rules_file: Path, cand_file: Path, reps: int) -> None:
    import spatial_rules
    import shapely_baseline as sb

    raw = rules_file.read_bytes()
    ruleset = spatial_rules.Ruleset.from_geojson(raw)
    rule_features = json.loads(raw.decode("utf-8"))["features"]
    cand_features = json.loads(cand_file.read_text())["features"]
    cand_bytes = cand_file.read_bytes()
    query = json.dumps(SPATIAL_QUERY)

    rule_geoms = sb.load_feature_geometries(rule_features)
    cand_geoms = sb.load_feature_geometries(cand_features)
    tree = sb.build_rule_index(rule_geoms)

    print(f"reference - {len(rule_features)} rules x {len(cand_features)} candidates (intersects, batch)")

    engine_count = engine_matched(ruleset, cand_bytes, query)
    naive_count = sb.scan_naive(cand_geoms, rule_geoms)
    indexed_count = sb.scan_indexed_array(cand_geoms, tree)
    if not (engine_count == naive_count == indexed_count):
        print(f"  !! mismatch: engine={engine_count} naive={naive_count} indexed={indexed_count}")
        raise SystemExit(1)
    print(f"matched = {engine_count} (engine, naive, indexed agree)")

    # Warmup each, then min-of-N.
    engine_matched(ruleset, cand_bytes, query)
    sb.scan_naive(cand_geoms, rule_geoms)
    sb.scan_indexed_array(cand_geoms, tree)

    naive_ms = min_of(lambda: sb.scan_naive(cand_geoms, rule_geoms), reps)
    indexed_ms = min_of(lambda: sb.scan_indexed_array(cand_geoms, tree), reps)
    engine_ms = min_of(lambda: engine_matched(ruleset, cand_bytes, query), reps)

    print()
    print(f"{'baseline':<28} {'time/batch':>12}   {'winner':>14}")
    print(table_row("Shapely naive (scan)", naive_ms, engine_ms))
    print(table_row("Shapely STRtree+prep", indexed_ms, engine_ms))
    print(f"{'engine (PyO3, full)':<28} {engine_ms:>10.2f} ms   {'baseline':>14}")

    idx_ratio = indexed_ms / engine_ms
    if idx_ratio < 1:
        winner = f"Shapely (STRtree+prep) wins by {speed_label(engine_ms / indexed_ms)} at this shape"
    else:
        winner = f"engine wins by {speed_label(idx_ratio)} vs indexed"
    print(f"\nhero: {winner}  (shapely {sb.shapely_version()})")


# ---- rules sweep (fixed candidates, sweep rule count) ----------------------

def run_rules_sweep(points: List[int], candidates_count: int, reps: int) -> None:
    import spatial_rules
    import shapely_baseline as sb

    query = json.dumps(SPATIAL_QUERY)
    rng = make_rng(0x51A70001)

    print()
    print(f"rules sweep - {candidates_count} candidates, rules={','.join(map(str, points))} reps={reps} (intersects)")
    print()
    print(f"{'rules':>8}  {'indexed (ms)':>14}  {'engine (ms)':>13}  {'winner':>14}  matched")

    for n in points:
        features = make_rule_features(n)
        cand_features = make_grid_candidates(candidates_count, n, rng)
        cand_bytes = json.dumps(to_collection(cand_features)).encode("utf-8")

        ruleset = spatial_rules.Ruleset.from_geojson(json.dumps(to_collection(features)))
        rule_geoms = sb.load_feature_geometries(features)
        cand_geoms = sb.load_feature_geometries(cand_features)
        tree = sb.build_rule_index(rule_geoms)

        engine_count = engine_matched(ruleset, cand_bytes, query)
        indexed_count = sb.scan_indexed_array(cand_geoms, tree)
        if engine_count != indexed_count:
            print(f"  !! mismatch at {n} rules: engine={engine_count} indexed={indexed_count}")
            raise SystemExit(1)

        engine_matched(ruleset, cand_bytes, query)
        sb.scan_indexed_array(cand_geoms, tree)
        indexed_ms = min_of(lambda: sb.scan_indexed_array(cand_geoms, tree), reps)
        engine_ms = min_of(lambda: engine_matched(ruleset, cand_bytes, query), reps)

        print(f"{n:>8}  {indexed_ms:>12.1f}  {engine_ms:>11.2f}  {vs_engine_cell(indexed_ms, engine_ms):>14}  {engine_count}")

    print("\nboth sides stay ~flat as rules grow (each has a real bbox index here); the gap is GEOS prepared relate beating the engine's relate loop, not index scaling")


# ---- main ------------------------------------------------------------------

def main() -> None:
    cfg = load_config()
    parser = argparse.ArgumentParser(description="Python benchmark: engine vs Shapely/GEOS")
    parser.add_argument("--reps", type=int, default=cfg.get("python", {}).get("reps", 3))
    parser.add_argument("--points", default=cfg.get("python", {}).get("points", "30,300,1000"))
    parser.add_argument("--candidates", type=int, default=cfg.get("python", {}).get("candidates", 1000))
    parser.add_argument("--rules-file", default=None, help="real-data mode (a GeoJSON boundary file)")
    args = parser.parse_args()

    def path_of(rel):
        p = Path(rel)
        return p if p.is_absolute() else (REPO_ROOT / p).resolve()

    rules_file = path_of(args.rules_file or cfg["global"]["paths"]["rulesFile"])
    cand_file = path_of(cfg["global"]["paths"]["candidatesFile"])

    print(f"spatial-rules python benchmark - engine vs Shapely")
    print(f"rules={rules_file.name}  candidates={cand_file.name}  reps={args.reps}  sweep rules={args.points}")

    run_reference(cfg, rules_file, cand_file, args.reps)
    if args.rules_file:
        print("\n(real-data mode: single file set - rules sweep skipped)")
    else:
        run_rules_sweep(
            [int(p) for p in str(args.points).split(",")],
            args.candidates,
            args.reps,
        )
    return None


if __name__ == "__main__":
    main()
