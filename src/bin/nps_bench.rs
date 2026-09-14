//! Fixed-node NNUE search NPS bench — the speed instrument for search
//! optimization candidates. Same positions, same node budget, every run:
//! the reported knps is comparable across builds and flags.
//!
//!   cargo run --release --bin nps_bench -- <fens.txt> [--net path] [--cal path]
//!       [--helper path] [--nodes 100000] [--repeat 1]
use cvs_bitboard_core::eval::{Nnue, ValueWeights};
use cvs_bitboard_core::search::{SearchOptions, Searcher};
use cvs_bitboard_core::Position;
use std::io::BufRead;
use std::time::Instant;

fn arg(args: &[String], flag: &str) -> Option<String> {
    args.iter()
        .position(|a| a == flag)
        .and_then(|i| args.get(i + 1).cloned())
}

fn main() {
    let args: Vec<String> = std::env::args().collect();
    let fens_path = &args[1];
    let nodes: u64 = arg(&args, "--nodes")
        .and_then(|s| s.parse().ok())
        .unwrap_or(100_000);
    let repeat: usize = arg(&args, "--repeat")
        .and_then(|s| s.parse().ok())
        .unwrap_or(1);

    let mut searcher = Searcher::new(ValueWeights::default(), None);
    if let Some(path) = arg(&args, "--net") {
        let mut n = Nnue::load(&path, true).expect("load net");
        if let Some(cal) = arg(&args, "--cal") {
            let text = std::fs::read_to_string(&cal).expect("read cal");
            let v: serde_json::Value = serde_json::from_str(&text).expect("parse cal");
            let pts: Vec<(f64, f64)> = v["points"]
                .as_array()
                .expect("points")
                .iter()
                .filter_map(|p| Some((p.as_array()?.first()?.as_f64()?, p.as_array()?.get(1)?.as_f64()?)))
                .collect();
            n.set_calibration(&pts);
        }
        searcher = Searcher::with_nnue(ValueWeights::default(), None, n);
    }
    if let Some(path) = arg(&args, "--helper") {
        let h = Nnue::load(&path, true).expect("load helper");
        searcher.set_helper_nnue(Some(h));
    }

    let fens: Vec<String> = std::io::BufReader::new(
        std::fs::File::open(fens_path).expect("open fens"),
    )
    .lines()
    .map_while(Result::ok)
    .filter(|l| !l.trim().is_empty())
    .collect();

    let opts = SearchOptions {
        depth: 40,
        max_nodes: Some(nodes),
        threads: 1,
        ..Default::default()
    };

    // Warm up (page in tables, JIT-less but caches/first-touch still matter).
    let mut total_nodes = 0u64;
    let mut total_ms = 0f64;
    for r in 0..repeat.max(1) {
        for fen in &fens {
            let mut pos = Position::from_fen(fen).expect("fen");
            let t0 = Instant::now();
            let res = searcher.search(&mut pos, opts);
            let t1 = Instant::now();
            if r + 1 == repeat.max(1) {
                total_nodes += res.telemetry.nodes;
                total_ms += (t1 - t0).as_secs_f64() * 1000.0;
            }
        }
    }
    let nps = total_nodes as f64 / (total_ms / 1000.0).max(1e-9);
    println!(
        "{} positions, {} nodes in {:.0} ms -> {:.0} knps",
        fens.len(),
        total_nodes,
        total_ms,
        nps / 1000.0
    );
}
