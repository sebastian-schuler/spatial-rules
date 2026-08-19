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

use criterion::{black_box, criterion_group, criterion_main, Criterion, Throughput};
use geo::{PreparedGeometry, Relate};
use spatial_rules_benchmarks::dataset;
use spatial_rules_core::{Candidate, CandidateClass, Ruleset, SpatialIndexKind};

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
                    if candidate.geometry.relate(*geometry).is_intersects() {
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
                    if candidate.geometry.relate(scan.geometry(rule_id)).is_intersects() {
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
                    if candidate.geometry.relate(rstar.geometry(rule_id)).is_intersects() {
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
                    if candidate.geometry.relate(prepared_rule).is_intersects() {
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
                    if candidate.geometry.relate(prepared.get(rule_id)).is_intersects() {
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

