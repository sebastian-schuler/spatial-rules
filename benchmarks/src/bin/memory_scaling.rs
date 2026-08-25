//! Memory scaling and lifecycle benchmark binary (memory-benchmark ticket 01).
//!
//!   memory_scaling --cell=1000,10 [--candidates=1000] [--query-batches=20]
//!                  [--replacements=20]
//!   memory_scaling --rules=1000,10000,100000 --vertices=10,100,1000 [...]
//!   memory_scaling --cells=1000x10,10000x100,100000x100 [...]
//!
//! With `--cell` the process measures exactly one grid cell and prints one
//! JSON object — this is also how aggregate mode runs each cell (fresh child
//! process via `current_exe`, so every cell sees a clean baseline and its own
//! peak counters). Aggregate mode spawns one child per cell and prints
//! `{"cells": [...]}`. `--cells=rules1xverts1,rules2xverts2,...` selects an
//! explicit cell list; without it the `--rules` × `--vertices` cross product
//! is used. Aggregate mode caps each cell's replacement count to a wall-time
//! budget (`capped_replacements`), so a default-grid run always finishes.

use spatial_rules_benchmarks::memory_scaling::{
    capped_replacements, measure_cell, CellOptions, Scale,
};

#[derive(Debug, Clone)]
struct Knobs {
    rules: Vec<usize>,
    vertices: Vec<usize>,
    opts: CellOptions,
}

fn parse_list(value: &str) -> Vec<usize> {
    value
        .split(',')
        .filter_map(|part| part.trim().parse().ok())
        .collect()
}

/// Pull `--flag=value` overrides; unknown flags are ignored.
fn knobs_from_args() -> Knobs {
    let mut knobs = Knobs {
        rules: vec![1_000, 10_000, 100_000],
        vertices: vec![10, 100, 1_000],
        opts: CellOptions::default(),
    };
    for arg in std::env::args().skip(1) {
        let Some((key, value)) = arg.strip_prefix("--").and_then(|rest| rest.split_once('=')) else {
            continue;
        };
        match key {
            "rules" => knobs.rules = parse_list(value),
            "vertices" => knobs.vertices = parse_list(value),
            "cell" => {}
            "candidates" => {
                if let Ok(n) = value.parse() {
                    knobs.opts.candidates = n;
                }
            }
            "query-batches" => {
                if let Ok(n) = value.parse() {
                    knobs.opts.query_batches = n;
                }
            }
            "replacements" => {
                if let Ok(n) = value.parse() {
                    knobs.opts.replacements = n;
                }
            }
            _ => {}
        }
    }
    knobs
}

fn main() {
    let mut cell: Option<(usize, usize)> = None;
    for arg in std::env::args().skip(1) {
        if let Some((key, value)) = arg.strip_prefix("--").and_then(|rest| rest.split_once('=')) {
            if key == "cell" {
                let parts: Vec<&str> = value.split(',').collect();
                if parts.len() == 2 {
                    if let (Ok(rules), Ok(vertices)) =
                        (parts[0].trim().parse(), parts[1].trim().parse())
                    {
                        cell = Some((rules, vertices));
                    }
                }
            }
        }
    }

    let knobs = knobs_from_args();

    if let Some((rules, vertices)) = cell {
        // Single-cell mode: measure in *this* process, print one JSON object.
        // Phase progress goes to stderr so long cells stay visible.
        spatial_rules_benchmarks::memory_scaling::set_progress(true);
        let report = measure_cell(
            Scale { rules, vertices },
            knobs.opts,
        )
        .unwrap_or_else(|error| {
            eprintln!("measure_cell failed: {error}");
            std::process::exit(1);
        });
        println!("{}", serde_json::to_string(&report).expect("serialize report"));
        return;
    }

    // Aggregate mode: one fresh child process per cell.
    let exe = std::env::current_exe().expect("current executable");
    let cells: Vec<Scale> = parse_cells_arg().unwrap_or_else(|| {
        knobs.vertices
            .iter()
            .flat_map(|&vertices| {
                knobs.rules.iter().map(move |&rules| Scale { rules, vertices })
            })
            .collect()
    });
    let mut child_cells = Vec::new();
    for &scale in &cells {
        // Cap swaps to a wall-time budget so the default grid finishes; the
        // child prints its own `[memory-scaling]` progress to stderr.
        let replacements = capped_replacements(scale, knobs.opts.replacements);
        eprintln!(
            "measuring cell: {} rules x {} vertices ({replacements} swaps)...",
            scale.rules, scale.vertices
        );
        let child = std::process::Command::new(&exe)
            .arg(format!("--cell={},{}", scale.rules, scale.vertices))
            .arg(format!("--candidates={}", knobs.opts.candidates))
            .arg(format!("--query-batches={}", knobs.opts.query_batches))
            .arg(format!("--replacements={replacements}"))
            .stdin(std::process::Stdio::null())
            .stdout(std::process::Stdio::piped())
            .stderr(std::process::Stdio::inherit())
            .spawn()
            .expect("spawn cell subprocess");
        let output = child.wait_with_output().expect("wait for cell subprocess");
        if !output.status.success() {
            eprint!("{}", String::from_utf8_lossy(&output.stderr));
            std::process::exit(output.status.code().unwrap_or(1));
        }
        let report: serde_json::Value =
            serde_json::from_slice(&output.stdout).expect("parse cell report");
        child_cells.push(report);
    }

    let aggregated = serde_json::json!({
        "workload": {
            "candidatesPerBatch": knobs.opts.candidates,
            "queryBatches": knobs.opts.query_batches,
            "replacements": knobs.opts.replacements,
        },
        "cells": child_cells,
    });
    println!(
        "{}",
        serde_json::to_string_pretty(&aggregated).expect("serialize aggregate report")
    );
}

/// Parse an explicit `--cells=rules1xverts1,rules2xverts2,...` list, if given.
fn parse_cells_arg() -> Option<Vec<Scale>> {
    for arg in std::env::args().skip(1) {
        let Some((key, value)) = arg.strip_prefix("--").and_then(|rest| rest.split_once('=')) else {
            continue;
        };
        if key != "cells" {
            continue;
        }
        let cells: Vec<Scale> = value
            .split(',')
            .filter_map(|part| {
                let mut it = part.trim().split('x');
                let rules = it.next()?.trim().parse().ok()?;
                let vertices = it.next()?.trim().parse().ok()?;
                if it.next().is_some() {
                    return None;
                }
                Some(Scale { rules, vertices })
            })
            .collect();
        if cells.is_empty() {
            eprintln!("--cells=... parsed to an empty list; falling back to --rules x --vertices");
            return None;
        }
        return Some(cells);
    }
    None
}
