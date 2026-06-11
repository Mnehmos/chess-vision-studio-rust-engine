//! CVS speed benchmark (brief Gate 2). Measures, on a fixed FEN suite:
//!   1. CVS feature extraction throughput (extract/sec, ns/call).
//!   2. Rung-2 extraction throughput (the baseline CVS sits on top of).
//!   3. Search NPS with CVS trace OFF (the shipping engine).
//!   4. Search NPS with CVS trace ON (per-leaf CVS extraction — upper-bound
//!      cost of geometry on every node).
//!
//!   cargo run --release --bin cvs_bench [-- <depth>]
use cvs_bitboard_core::eval::cvs_features::extract_cvs_ids_into;
use cvs_bitboard_core::eval::extract_rung2;
use cvs_bitboard_core::search::{SearchOptions, Searcher};
use cvs_bitboard_core::Position;
use std::time::Instant;

const SUITE: &[(&str, &str)] = &[
    (
        "startpos",
        "rnbqkbnr/pppppppp/8/8/8/8/PPPPPPPP/RNBQKBNR w KQkq - 0 1",
    ),
    (
        "kiwipete",
        "r3k2r/p1ppqpb1/bn2pnp1/3PN3/1p2P3/2N2Q1p/PPPBBPPP/R3K2R w KQkq - 0 1",
    ),
    (
        "midgame",
        "4r1k1/1p3pp1/p1p3rp/P1Qnq3/1PB5/4P3/5PPP/3R1RK1 b - - 5 27",
    ),
    (
        "preBd6",
        "r1b3nr/1pp1bkpp/p1n5/1q3p2/3P4/B1PNQ1P1/P4PBP/RN2R1K1 b - - 3 19",
    ),
    ("endgame", "8/5pk1/8/6p1/8/6P1/4QPK1/8 w - - 0 40"),
];

fn main() {
    let depth: u32 = std::env::args()
        .nth(1)
        .and_then(|s| s.parse().ok())
        .unwrap_or(8);
    let positions: Vec<Position> = SUITE
        .iter()
        .map(|(_, f)| Position::from_fen(f).unwrap())
        .collect();

    // --- 1 & 2: extraction throughput ---
    const ITERS: u32 = 200_000;
    let mut sink = 0u64;
    let mut buf = Vec::with_capacity(32);
    let t = Instant::now();
    for _ in 0..ITERS {
        for p in &positions {
            extract_cvs_ids_into(p, &mut buf);
            sink += buf.len() as u64;
        }
    }
    let cvs_ns = t.elapsed().as_nanos() as f64 / (ITERS as f64 * positions.len() as f64);

    let t = Instant::now();
    for _ in 0..ITERS {
        for p in &positions {
            let r = extract_rung2(p);
            sink += r.king_danger.to_bits();
        }
    }
    let rung2_ns = t.elapsed().as_nanos() as f64 / (ITERS as f64 * positions.len() as f64);

    println!(
        "# CVS speed bench (depth {depth}, suite {} positions)\n",
        positions.len()
    );
    println!("## Extraction throughput");
    println!("| Path | ns/call | calls/sec | vs rung2 |");
    println!("|---|---:|---:|---:|");
    println!(
        "| rung2 (baseline)   | {:.0} | {:.2}M | 1.00x |",
        rung2_ns,
        1e3 / rung2_ns
    );
    println!(
        "| CVS features (Tier1) | {:.0} | {:.2}M | {:.2}x |",
        cvs_ns,
        1e3 / cvs_ns,
        cvs_ns / rung2_ns
    );

    // --- 3 & 4: search NPS, CVS trace off vs on ---
    println!("\n## Search NPS (depth {depth})");
    println!("| Position | trace OFF nps | trace ON nps | slowdown | trace feats |");
    println!("|---|---:|---:|---:|---:|");
    let mut tot_off_nodes = 0u64;
    let mut tot_off_ms = 0u64;
    let mut tot_on_nodes = 0u64;
    let mut tot_on_ms = 0u64;
    for (name, fen) in SUITE {
        let run = |trace: bool| {
            let mut pos = Position::from_fen(fen).unwrap();
            let mut s = Searcher::new(Default::default(), None);
            let r = s.search(
                &mut pos,
                SearchOptions {
                    depth,
                    cvs_trace: trace,
                    ..Default::default()
                },
            );
            (
                r.telemetry.nodes,
                r.telemetry.elapsed_ms.max(1),
                r.telemetry.cvs_trace_features,
            )
        };
        let (off_n, off_ms, _) = run(false);
        let (on_n, on_ms, feats) = run(true);
        let off_nps = off_n * 1000 / off_ms;
        let on_nps = on_n * 1000 / on_ms;
        tot_off_nodes += off_n;
        tot_off_ms += off_ms;
        tot_on_nodes += on_n;
        tot_on_ms += on_ms;
        println!(
            "| {name} | {} | {} | {:.2}x | {} |",
            off_nps,
            on_nps,
            on_nps as f64 / off_nps.max(1) as f64,
            feats
        );
    }
    let off_nps = tot_off_nodes * 1000 / tot_off_ms.max(1);
    let on_nps = tot_on_nodes * 1000 / tot_on_ms.max(1);
    println!(
        "| **TOTAL** | **{}** | **{}** | **{:.2}x** | |",
        off_nps,
        on_nps,
        on_nps as f64 / off_nps.max(1) as f64
    );
    println!("\nsink={sink}");
}
