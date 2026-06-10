//! Search benchmark + TS comparison harness — the same three positions and
//! depths as the TS `bench:search` (forensic #549, startpos, midgame-r1), under
//! default and trained mixed weights. Prints time / nodes / qNodes / extensions /
//! TT hits / nps / best / score per cell, plus PARITY lines in the TS bench
//! format for side-by-side diffing.
//!
//!   cargo run --release --bin search_bench [-- <base-weights.json> <rung2-weights.json> [maxdepth]]
use cvs_bitboard_core::eval::{Rung2Weights, ValueWeights};
use cvs_bitboard_core::search::{SearchOptions, Searcher};
use cvs_bitboard_core::Position;

const POSITIONS: &[(&str, &str)] = &[
    (
        "549-d4d5-blunder",
        "5r2/pp5R/1kp3p1/6b1/4P1b1/1BNP2P1/PPP4P/1K6 w - - 1 22",
    ),
    (
        "startpos",
        "rnbqkbnr/pppppppp/8/8/8/8/PPPPPPPP/RNBQKBNR w KQkq - 0 1",
    ),
    (
        "midgame-r1",
        "4r1k1/1p3pp1/p1p3rp/P1Qnq3/1PB5/4P3/5PPP/3R1RK1 b - - 5 27",
    ),
];

fn main() {
    let args: Vec<String> = std::env::args().collect();
    let (base, rung2): (ValueWeights, Option<Rung2Weights>) = if args.len() >= 3 {
        let b = serde_json::from_str(&std::fs::read_to_string(&args[1]).expect("base weights"))
            .expect("parse base weights");
        let r = serde_json::from_str(&std::fs::read_to_string(&args[2]).expect("rung2 weights"))
            .expect("parse rung2 weights");
        (b, Some(r))
    } else {
        (ValueWeights::default(), None)
    };
    let max_depth: u32 = args.get(3).and_then(|s| s.parse().ok()).unwrap_or(6);

    let engines: Vec<(&str, ValueWeights, Option<Rung2Weights>)> = vec![
        ("default", ValueWeights::default(), None),
        ("mixed", base, rung2),
    ];

    println!("# Rust search bench (depths 2..={max_depth})");
    println!("| Engine | Position | Depth | Time(ms) | Nodes | qNodes | QuietExt | TThits | Nodes/s | Best | Score |");
    println!("|---|---|---:|---:|---:|---:|---:|---:|---:|---|---:|");
    let mut parity: Vec<String> = Vec::new();
    for (name, w, r2) in &engines {
        if *name == "mixed" && r2.is_none() {
            continue; // no weight files supplied
        }
        for (pname, fen) in POSITIONS {
            for depth in 2..=max_depth {
                let mut pos = Position::from_fen(fen).expect("valid FEN");
                let mut s = Searcher::new(*w, *r2);
                let r = s.search(
                    &mut pos,
                    SearchOptions {
                        depth,
                        ..Default::default()
                    },
                );
                let t = r.telemetry;
                let ms = t.elapsed_ms.max(1);
                let nps = t.nodes * 1000 / ms;
                let best = r
                    .best_move
                    .map(|m| m.to_uci())
                    .unwrap_or_else(|| "(none)".into());
                println!(
                    "| {name} | {pname} | {depth} | {} | {} | {} | {} | {} | {nps} | {best} | {} |",
                    t.elapsed_ms,
                    t.nodes,
                    t.q_nodes,
                    t.quiet_check_extensions,
                    t.tt_hits,
                    r.score_cp
                );
                parity.push(format!(
                    "PARITY {name}|{pname}|d{depth} => {best}@{}",
                    r.score_cp
                ));
            }
        }
    }
    println!();
    for p in &parity {
        println!("{p}");
    }
}
