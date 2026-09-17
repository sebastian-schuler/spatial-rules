# Smoke test for the Python binding — run with the module built:
#   maturin develop (in python/, against a Python env) then
#   python -m pytest tests/test_smoke.py
#
# Mirrors the node/wasm/integration smokes: the controlled-ruleset literals
# plus the production `~1k×30` matched count (481).

import json
from pathlib import Path

import pytest

import spatial_rules

RULES = {
    "type": "FeatureCollection",
    "features": [
        {
            "type": "Feature",
            "id": "zone-a",
            "priority": 10,
            "properties": {"active": True, "name": "a", "shared": "from-a", "priority": 999, "daysOfWeek": 1, "startHour": 0, "endHour": 24},
            "geometry": {"type": "Polygon", "coordinates": [[[0, 0], [0, 10], [10, 10], [10, 0], [0, 0]]]},
        },
        {
            "type": "Feature",
            "id": "zone-b",
            "priority": 5,
            "properties": {"active": False, "name": "b", "daysOfWeek": 2, "startHour": 0, "endHour": 24},
            "geometry": {"type": "Polygon", "coordinates": [[[100, 100], [100, 110], [110, 110], [110, 100], [100, 100]]]},
        },
        {
            "type": "Feature",
            "id": "zone-c",
            "priority": 20,
            "properties": {"active": True, "name": "c"},
            "geometry": {"type": "Polygon", "coordinates": [[[2, 2], [2, 12], [12, 12], [12, 2], [2, 2]]]},
        },
    ],
}

CANDIDATES = {
    "type": "FeatureCollection",
    "features": [
        {"type": "Feature", "id": "inside", "properties": {}, "geometry": {"type": "Polygon", "coordinates": [[[2, 2], [2, 4], [4, 4], [4, 2], [2, 2]]]}},
        {"type": "Feature", "id": "far", "properties": {}, "geometry": {"type": "Polygon", "coordinates": [[[50, 50], [50, 60], [60, 60], [60, 50], [50, 50]]]}},
        {"type": "Feature", "id": "invalid", "properties": {}, "geometry": {"type": "Polygon", "coordinates": [[[0, 0], [10, 10], [0, 10], [10, 0], [0, 0]]]}},
    ],
}

POINT_PAIR = {
    "type": "FeatureCollection",
    "features": [
        {"type": "Feature", "id": "pt-in", "properties": {}, "geometry": {"type": "Point", "coordinates": [5, 5]}},
        {"type": "Feature", "id": "pt-out", "properties": {}, "geometry": {"type": "Point", "coordinates": [50, 50]}},
    ],
}


def test_query_mask():
    rs = spatial_rules.Ruleset.from_geojson(RULES)
    mask = rs.query(CANDIDATES, {"spatial": {"predicate": "intersects"}})
    assert mask == [1, 0, 2]


def test_query_rich_string_rule_ids():
    rs = spatial_rules.Ruleset.from_geojson(RULES)
    rich = rs.query_rich(CANDIDATES, {"spatial": {"predicate": "intersects"}})
    assert rich[0]["outcome"] == "matched"
    assert rich[0]["ruleIds"] == ["zone-a", "zone-c"]
    assert rich[1]["outcome"] == "notMatched"
    assert rich[2]["outcome"] == "invalid"


def test_within_distance():
    rs = spatial_rules.Ruleset.from_geojson(RULES)
    mask = rs.query(POINT_PAIR, {"spatial": {"predicate": "withinDistance", "distance": 100}})
    assert mask == [1, 0]


def test_temporal_active_at():
    rs = spatial_rules.Ruleset.from_geojson(RULES)
    active_at = {"daysOfWeek": "daysOfWeek", "startHour": "startHour", "endHour": "endHour"}
    query = {"spatial": {"predicate": "intersects"}, "where": {"$activeAt": active_at}}
    assert rs.query(CANDIDATES, {**query, "at": "2026-08-24T10:00"}) == [1, 0, 2]
    assert rs.query(CANDIDATES, {**query, "at": "2026-08-25T10:00"}) == [0, 0, 2]


def test_aggregate():
    rs = spatial_rules.Ruleset.from_geojson(RULES)
    rich = rs.query_rich(CANDIDATES, {"spatial": {"predicate": "intersects"}, "aggregate": {"count": True, "coverage": True}})
    assert rich[0]["aggregate"]["count"] == 2
    assert rich[0]["aggregate"]["coverage"] > 0.9
    assert "aggregate" not in rich[1]


def test_resolve():
    rs = spatial_rules.Ruleset.from_geojson(RULES)
    assert rs.resolve(CANDIDATES, {"spatial": {"predicate": "intersects"}}) == [1, 0, 2]
    resolved = rs.resolve_rich(CANDIDATES, {"spatial": {"predicate": "intersects"}})
    assert resolved[0]["outcome"] == "resolved"
    assert resolved[0]["winner"] == "zone-c"
    assert resolved[0]["values"]["shared"] == "from-a"
    assert resolved[0]["values"]["daysOfWeek"] == 1
    assert [a["ruleId"] for a in resolved[0]["applicable"]] == ["zone-c", "zone-a"]
    resolve_agg = rs.resolve_rich(CANDIDATES, {"spatial": {"predicate": "intersects"}, "aggregate": {"count": True}})
    assert resolve_agg[0]["aggregate"]["count"] == 2


def test_replace_canonical_stats():
    rs = spatial_rules.Ruleset.from_geojson(RULES)
    stats = rs.stats()
    assert stats["version"] == 1
    assert stats["ruleCount"] == 3
    report = rs.replace(RULES)
    assert report["version"] == 2
    assert report["ruleCount"] == 3
    canonical = rs.to_canonical()
    assert isinstance(canonical, list)
    assert len(canonical) == 3
    assert canonical[0]["id"] == "zone-a"


def test_input_types():
    rs = spatial_rules.Ruleset.from_geojson(json.dumps(RULES))
    mask = rs.query(json.dumps(CANDIDATES), json.dumps({"spatial": {"predicate": "intersects"}}))
    assert mask == [1, 0, 2]


def test_error_carries_sr_code():
    rs = spatial_rules.Ruleset.from_geojson(RULES)
    with pytest.raises(spatial_rules.SpatialRulesError) as exc:
        rs.query(CANDIDATES, "not json")
    assert "SR_INVALID_QUERY" in str(exc.value)


def test_input_conversion_errors_map_to_sr_code():
    rs = spatial_rules.Ruleset.from_geojson(json.dumps(RULES))
    # Non-UTF-8 candidate bytes must raise SpatialRulesError (SR_*), not a bare
    # Python exception, matching the documented error contract.
    with pytest.raises(spatial_rules.SpatialRulesError) as exc:
        rs.query(b"\xff\xfe invalid utf-8", json.dumps({"spatial": {"predicate": "intersects"}}))
    assert "SR_INVALID_GEOJSON" in str(exc.value)


def test_from_geojson_error_carries_sr_code():
    with pytest.raises(spatial_rules.SpatialRulesError) as exc:
        spatial_rules.Ruleset.from_geojson("not json")
    assert "SR_INVALID_GEOJSON" in str(exc.value)


def test_production_matched_count():
    repo_root = Path(__file__).resolve().parents[2]
    production_rules = json.loads((repo_root / "benchmarks/data/rules.geojson").read_text())
    production_candidates = json.loads((repo_root / "benchmarks/data/candidates.geojson").read_text())
    rs = spatial_rules.Ruleset.from_geojson(production_rules)
    mask = rs.query(production_candidates, {"spatial": {"predicate": "intersects"}})
    assert len(mask) == len(production_candidates["features"])
    assert sum(1 for value in mask if value == 1) == 481