//! Algorithm-ladder benchmarks (ticket 12, §32).
//!
//! - **B** — Rust naive: every candidate × every rule, exact DE-9IM only.
//! - **C** — Rust + bounding-box filtering (linear envelope scan).
//! - **D** — Rust + spatial index (`rstar` bulk-load, the default).
//! - ruleset build time (§31).
//!
//! **E/F** (prepared geometries) wait on a prepare path in core (research 03);
//! **A** (the existing JS implementation) lives outside this repo and is
//! measured by the turf.js cross-check harness.

use criterion::{black_box, criterion_group, criterion_main, Criterion, Throughput};
use geo::Relate;
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

    let mut group = criterion.benchmark_group("batch_query");
    group.throughput(Throughput::Elements(candidate_count));

    group.bench_function("B_naive_candidate_times_rule", |bencher| {
        bencher.iter(|| {
            let mut matches = 0usize;
            for candidate in &candidates {
                for index in 0..rstar.len() {
                    let rule_geometry = rstar.geometry(RuleId(index as u32));
                    if candidate.geometry.relate(rule_geometry).is_intersects() {
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

    group.finish();

    let mut build = criterion.benchmark_group("ruleset_build");
    build.bench_function("build_30_rules", |bencher| {
        bencher.iter(|| black_box(Ruleset::build(black_box(rules.clone()))))
    });
    build.finish();
}

criterion_group!(benches, bench_ladder);
criterion_main!(benches);
