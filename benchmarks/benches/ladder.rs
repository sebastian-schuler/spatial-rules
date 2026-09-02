//! Algorithm-ladder benchmarks (ticket 12, §32).
//!
//! Every rung drives the engine through its public seams (rule source,
//! envelope query, prepared form by opaque id — architecture-hardening 04) and
//! differs from its neighbour by exactly one variable:
//!
//! - **B** — naive: every candidate × every rule, exact DE-9IM, unprepared.
//! - **C** — + bounding-box filtering (linear envelope scan), unprepared.
//! - **D** — + spatial index (`rstar` bulk-load, the default), unprepared.
//! - **E** — prepared geometries, no bbox.
//! - **F** — prepared geometries + rstar bbox.
//!
//! So the two levers are isolated: bbox filter (B→C), index kind (C→D),
//! prepared geometries (B→E, and D→F with the index held constant).
//!
//! **A** (the existing JS implementation) is the turf.js baseline in
//! `bun run bench perf` (`benchmarks/js/server-bench.mjs`).
//!
//! The surfaces that shipped after the ladder (P1 resolution, P2 temporal +
//! `withinDistance`, aggregation) are additive on top of the F mask path and
//! are measured in `bench_surfaces` — each with a mask/rich-path baseline, so
//! the cost of each surface is the delta over its baseline.

use criterion::{black_box, criterion_group, criterion_main, Criterion, Throughput};
use geo::{PreparedGeometry, Relate};
use spatial_rules_benchmarks::dataset;
use spatial_rules_core::{
    AggregateSpec, Candidate, CandidateClass, CandidateOutcome, Query, Ruleset, SpatialIndexKind,
    SpatialPredicate,
};

/// The precomputed candidate envelope (architecture-hardening 01). The ladder
/// reads it instead of re-deriving `bounding_rect` per candidate.
fn candidate_envelope(candidate: &Candidate) -> geo::Rect<f64> {
    match candidate.class() {
        CandidateClass::Valid { envelope } => *envelope,
        CandidateClass::Invalid { .. } => unreachable!("dataset candidates are valid"),
    }
}

fn bench_ladder(criterion: &mut Criterion) {
    let rules = dataset::rules();
    let candidates = dataset::candidates();
    let rstar = Ruleset::build_with(rules.clone(), SpatialIndexKind::RStar).expect("rstar ruleset");
    let scan =
        Ruleset::build_with(rules.clone(), SpatialIndexKind::LinearScan).expect("scan ruleset");
    let candidate_count = candidates.len() as u64;

    // Rule geometries in ruleset order via the `RuleSource` seam (ADR-0002),
    // and prepared forms via the `PreparedRuleGeometries` seam indexed by
    // opaque `RuleId` (architecture-hardening 04) — no positional id→index map
    // is rebuilt.
    let geometries: Vec<&geo::Geometry<f64>> = rstar
        .rules()
        .iter()
        .map(|(_, geometry, _)| geometry)
        .collect();
    let prepared = rstar.prepared();

    let mut group = criterion.benchmark_group("batch_query");
    group.throughput(Throughput::Elements(candidate_count));

    group.bench_function("B_naive_candidate_times_rule", |bencher| {
        bencher.iter(|| {
            let mut matches = 0usize;
            for candidate in &candidates {
                for geometry in &geometries {
                    if candidate.geometry().relate(*geometry).is_intersects() {
                        matches += 1;
                    }
                }
            }
            black_box(matches)
        })
    });

    group.bench_function("C_linear_scan_bbox", |bencher| {
        let mut hits = Vec::new();
        bencher.iter(|| {
            let mut matches = 0usize;
            for candidate in &candidates {
                let bbox = candidate_envelope(candidate);
                scan.query_envelope_into(&bbox, &mut hits);
                for &rule_id in &hits {
                    if candidate
                        .geometry()
                        .relate(scan.geometry(rule_id).expect("rule id minted by ruleset"))
                        .is_intersects()
                    {
                        matches += 1;
                    }
                }
            }
            black_box(matches)
        })
    });

    group.bench_function("D_rstar_bbox", |bencher| {
        let mut hits = Vec::new();
        bencher.iter(|| {
            let mut matches = 0usize;
            for candidate in &candidates {
                let bbox = candidate_envelope(candidate);
                rstar.query_envelope_into(&bbox, &mut hits);
                for &rule_id in &hits {
                    if candidate
                        .geometry()
                        .relate(rstar.geometry(rule_id).expect("rule id minted by ruleset"))
                        .is_intersects()
                    {
                        matches += 1;
                    }
                }
            }
            black_box(matches)
        })
    });

    group.bench_function("E_prepared_naive", |bencher| {
        bencher.iter(|| {
            let mut matches = 0usize;
            for candidate in &candidates {
                for prepared_rule in prepared.iter() {
                    if candidate.geometry().relate(prepared_rule).is_intersects() {
                        matches += 1;
                    }
                }
            }
            black_box(matches)
        })
    });

    group.bench_function("F_prepared_rstar_bbox", |bencher| {
        let mut hits = Vec::new();
        bencher.iter(|| {
            let mut matches = 0usize;
            for candidate in &candidates {
                let bbox = candidate_envelope(candidate);
                rstar.query_envelope_into(&bbox, &mut hits);
                for &rule_id in &hits {
                    if candidate
                        .geometry()
                        .relate(prepared.get(rule_id).expect("rule id minted by ruleset"))
                        .is_intersects()
                    {
                        matches += 1;
                    }
                }
            }
            black_box(matches)
        })
    });

    group.finish();

    let mut build = criterion.benchmark_group("ruleset_build");
    build.bench_function("build_30_rules", |bencher| {
        bencher.iter(|| black_box(Ruleset::build(black_box(rules.clone()))))
    });
    build.finish();

    let mut prepare = criterion.benchmark_group("prepare");
    prepare.bench_function("prepare_30_rules", |bencher| {
        bencher.iter(|| {
            black_box(
                geometries
                    .iter()
                    .map(|geometry| PreparedGeometry::from(*geometry))
                    .collect::<Vec<_>>(),
            )
        })
    });
    prepare.finish();
}

/// Benchmarks for the surfaces shipped after the ladder's mask rungs (P1
/// resolution, P2 temporal + `withinDistance`, aggregation). All four paths are
/// **additive** on top of the F mask relate loop; each group measures its cell
/// against a mask/rich-path baseline so the surface's own cost is the delta.
///
/// - **resolution** — `resolve_mask` (the admission loop; no winner/values
///   materialised) and `resolve_full` (the ordered applicable-set gather +
///   precedence sort + winner + first-provider-wins values) over the
///   `~1k×30` workload.
/// - **withinDistance** — a geofencing workload: 1,000 point candidates at the
///   polygon candidates' envelope centers (`dataset::point_candidates`), radius
///   100 km — the bounding-circle pre-filter over the R-tree plus the exact
///   haversine minimum-distance confirm, vs the same points through the mask.
/// - **temporal** — a `$activeAt` where clause at a Monday-10:00 `at` over the
///   window-bearing rules (`dataset::rules_with_windows`): the per-rule window
///   scan cost. The dataset's windows all admit at that `at`, so the scan runs
///   over every touched rule with no pruning benefit — the delta vs the
///   no-where mask is the pure window-scan cost, and the masks are
///   byte-identical.
/// - **aggregation** — the rich-path aggregate over the applicable set: the
///   count + numeric (`priority`) fold, and the union-coverage geodesic measure
///   (the expensive BooleanOps/geodesic cell), each vs the mask and the rich
///   `query` gather.
fn bench_surfaces(criterion: &mut Criterion) {
    let candidates = dataset::candidates();
    let points = dataset::point_candidates();
    let rstar =
        Ruleset::build_with(dataset::rules(), SpatialIndexKind::RStar).expect("rstar ruleset");
    let window_rstar = Ruleset::build_with(dataset::rules_with_windows(), SpatialIndexKind::RStar)
        .expect("window rstar ruleset");
    let candidate_count = candidates.len() as u64;

    let intersects = Query::new(SpatialPredicate::Intersects);
    let within_distance =
        Query::new(SpatialPredicate::WithinDistance).with_distance(100_000.0);
    let temporal = Query::new(SpatialPredicate::Intersects)
        .with_where(dataset::active_at_clause())
        .with_at(dataset::monday_ten());
    let count_numeric = AggregateSpec::from_json(&serde_json::json!({
        "count": true,
        "min": "priority", "max": "priority", "sum": "priority", "avg": "priority",
    }))
    .expect("count/numeric aggregate spec");
    let coverage =
        AggregateSpec::from_json(&serde_json::json!({ "coverage": true })).expect("coverage spec");

    let mut resolve = criterion.benchmark_group("batch_resolve");
    resolve.throughput(Throughput::Elements(candidate_count));
    resolve.bench_function("mask_baseline", |bencher| {
        bencher.iter(|| black_box(rstar.query_mask(&candidates, &intersects)))
    });
    resolve.bench_function("resolve_mask", |bencher| {
        bencher.iter(|| black_box(rstar.resolve_mask(&candidates, &intersects)))
    });
    resolve.bench_function("resolve_full", |bencher| {
        bencher.iter(|| black_box(rstar.resolve(&candidates, &intersects)))
    });
    resolve.finish();

    let mut within = criterion.benchmark_group("batch_within_distance");
    within.throughput(Throughput::Elements(candidate_count));
    within.bench_function("mask_baseline", |bencher| {
        bencher.iter(|| black_box(rstar.query_mask(&points, &intersects)))
    });
    within.bench_function("within_distance_mask", |bencher| {
        bencher.iter(|| black_box(rstar.query_mask(&points, &within_distance)))
    });
    within.bench_function("within_distance_full", |bencher| {
        bencher.iter(|| black_box(rstar.query(&points, &within_distance)))
    });
    within.finish();

    let mut temporal_group = criterion.benchmark_group("batch_temporal");
    temporal_group.throughput(Throughput::Elements(candidate_count));
    temporal_group.bench_function("mask_no_where", |bencher| {
        bencher.iter(|| black_box(window_rstar.query_mask(&candidates, &intersects)))
    });
    temporal_group.bench_function("temporal_active_at", |bencher| {
        bencher.iter(|| black_box(window_rstar.query_mask(&candidates, &temporal)))
    });
    temporal_group.bench_function("temporal_active_at_full", |bencher| {
        bencher.iter(|| black_box(window_rstar.query(&candidates, &temporal)))
    });
    temporal_group.finish();

    let mut aggregate = criterion.benchmark_group("batch_aggregation");
    aggregate.throughput(Throughput::Elements(candidate_count));
    aggregate.bench_function("mask_baseline", |bencher| {
        bencher.iter(|| black_box(rstar.query_mask(&candidates, &intersects)))
    });
    aggregate.bench_function("query_rich_baseline", |bencher| {
        bencher.iter(|| black_box(rstar.query(&candidates, &intersects)))
    });
    // The aggregate rides the rich outcome (ADR-0018): query once with the spec
    // and read `aggregate` off each matched outcome — the realistic path.
    let query_count = Query::new(SpatialPredicate::Intersects).with_aggregate(count_numeric);
    let query_coverage = Query::new(SpatialPredicate::Intersects).with_aggregate(coverage);
    aggregate.bench_function("aggregate_count_numeric", |bencher| {
        bencher.iter(|| {
            let outcomes = rstar.query(&candidates, &query_count);
            let mut total = 0u64;
            for outcome in &outcomes {
                if let CandidateOutcome::Matched {
                    aggregate: Some(aggregate),
                    ..
                } = outcome
                {
                    total += aggregate.count.unwrap_or(0) as u64;
                }
            }
            black_box(total)
        })
    });
    aggregate.bench_function("aggregate_coverage", |bencher| {
        bencher.iter(|| {
            let outcomes = rstar.query(&candidates, &query_coverage);
            let mut total = 0.0f64;
            for outcome in &outcomes {
                if let CandidateOutcome::Matched {
                    aggregate: Some(aggregate),
                    ..
                } = outcome
                {
                    total += aggregate.coverage.unwrap_or(0.0);
                }
            }
            black_box(total)
        })
    });
    aggregate.finish();
}

criterion_group!(benches, bench_ladder, bench_surfaces);
criterion_main!(benches);

