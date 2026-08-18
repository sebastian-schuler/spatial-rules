//! Algorithm-ladder benchmarks (ticket 12, §32).
//!
//! - **B** — Rust naive: every candidate × every rule, exact DE-9IM only.
//! - **C** — Rust + bounding-box filtering (linear envelope scan).
//! - **D** — Rust + spatial index (`rstar` bulk-load, the default).
//! - **E** — Rust + prepared geometries (naive, no bbox).
//! - **F** — Rust + spatial index + prepared geometries.
//! - ruleset build time and per-worker preparation cost (§31).
//!
//! **A** (the existing JS implementation) is the turf.js baseline in
//! `benchmarks/js/perf.mjs`.

use criterion::{black_box, criterion_group, criterion_main, Criterion, Throughput};
use geo::{BoundingRect, PreparedGeometry, Relate};
use spatial_rules_benchmarks::dataset;
use spatial_rules_core::{Query, RuleId, Ruleset, SpatialIndexKind, SpatialPredicate};

fn bench_ladder(criterion: &mut Criterion) {
    let rules = dataset::rules();
    let candidates = dataset::candidates();
    let rstar = Ruleset::build_with(rules.clone(), SpatialIndexKind::RStar).expect("rstar ruleset");
    let scan =
        Ruleset::build_with(rules.clone(), SpatialIndexKind::LinearScan).expect("scan ruleset");
    let query = Query::new(SpatialPredicate::Intersects);
    let candidate_count = candidates.len() as u64;

    // Rule geometries in ruleset order, prepared once outside the timed loop.
    // `prepared_by_id` maps an opaque RuleId to its position so the
    // bbox-filtered prepared stage (F) reuses the same prepared slice without
    // reconstructing a rule's position.
    let geometries = rstar.rule_geometries();
    let prepared: Vec<PreparedGeometry<'_, &geo::Geometry<f64>>> = geometries
        .iter()
        .map(|geometry| PreparedGeometry::from(*geometry))
        .collect();
    let prepared_by_id: std::collections::HashMap<RuleId, usize> = rstar
        .rule_ids()
        .iter()
        .enumerate()
        .map(|(index, id)| (*id, index))
        .collect();

    let mut group = criterion.benchmark_group("batch_query");
    group.throughput(Throughput::Elements(candidate_count));

    group.bench_function("B_naive_candidate_times_rule", |bencher| {
        bencher.iter(|| {
            let mut matches = 0usize;
            for candidate in &candidates {
                for geometry in &geometries {
                    if candidate.geometry.relate(*geometry).is_intersects() {
                        matches += 1;
                    }
                }
            }
            black_box(matches)
        })
    });

    group.bench_function("C_linear_scan_bbox", |bencher| {
        bencher.iter(|| black_box(scan.query(black_box(&candidates), &query)))
    });

    group.bench_function("D_rstar_bbox", |bencher| {
        bencher.iter(|| black_box(rstar.query(black_box(&candidates), &query)))
    });

    group.bench_function("E_prepared_naive", |bencher| {
        bencher.iter(|| {
            let mut matches = 0usize;
            for candidate in &candidates {
                for prepared_rule in &prepared {
                    if candidate.geometry.relate(prepared_rule).is_intersects() {
                        matches += 1;
                    }
                }
            }
            black_box(matches)
        })
    });

    group.bench_function("F_prepared_rstar_bbox", |bencher| {
        bencher.iter(|| {
            let mut matches = 0usize;
            for candidate in &candidates {
                let bbox = candidate.geometry.bounding_rect().expect("candidate bbox");
                for rule_id in rstar.query_envelope(&bbox) {
                    if candidate
                        .geometry
                        .relate(&prepared[prepared_by_id[&rule_id]])
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

criterion_group!(benches, bench_ladder);
criterion_main!(benches);
