//! Writes the benchmark dataset to GeoJSON files (ticket 12).

use spatial_rules_benchmarks::dataset;

fn main() {
    let out_dir = std::env::args()
        .nth(1)
        .unwrap_or_else(|| "benchmarks/data".to_string());
    std::fs::create_dir_all(&out_dir).expect("create output directory");
    std::fs::write(
        format!("{out_dir}/rules.geojson"),
        dataset::rules_geojson(),
    )
    .expect("write rules.geojson");
    std::fs::write(
        format!("{out_dir}/candidates.geojson"),
        dataset::candidates_geojson(),
    )
    .expect("write candidates.geojson");
    println!(
        "wrote {} rules and {} candidates to {out_dir}/",
        dataset::rules().len(),
        dataset::candidates().len()
    );
}
