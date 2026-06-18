//! CVS speed benchmark (brief Gate 2). Measures, on a fixed FEN suite:
//!   1. CVS feature extraction throughput (extract/sec, ns/call).
//!   2. Rung-2 extraction throughput (the baseline CVS sits on top of).
//!   3. Search NPS with CVS trace OFF (the shipping engine).
//!   4. Search NPS with CVS trace ON (per-leaf CVS extraction — upper-bound
//!      cost of geometry on every node).
//!
//!   cargo run --release --bin cvs_bench [-- <depth>]
use cvs_bitboard_core::eval::cvs_features::{extract_cvs_ids_into, extract_cvs_core_ids_into};
use cvs_bitboard_core::eval::rung2::{extract_rung2, extract_rung2_core};
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
    
    // 1. CVS Full Features
    let t = Instant::now();
    for _ in 0..ITERS {
        for p in &positions {
            extract_cvs_ids_into(p, &mut buf);
            sink += buf.len() as u64;
        }
    }
    let cvs_ns = t.elapsed().as_nanos() as f64 / (ITERS as f64 * positions.len() as f64);

    // 2. CVS Core Features
    let t = Instant::now();
    for _ in 0..ITERS {
        for p in &positions {
            extract_cvs_core_ids_into(p, &mut buf);
            sink += buf.len() as u64;
        }
    }
    let cvs_core_ns = t.elapsed().as_nanos() as f64 / (ITERS as f64 * positions.len() as f64);

    // 3. Rung2 Full
    let t = Instant::now();
    for _ in 0..ITERS {
        for p in &positions {
            let r = extract_rung2(p);
            sink += r.king_danger.to_bits();
        }
    }
    let rung2_ns = t.elapsed().as_nanos() as f64 / (ITERS as f64 * positions.len() as f64);

    // 4. Rung2 Core
    let t = Instant::now();
    for _ in 0..ITERS {
        for p in &positions {
            let r = extract_rung2_core(p);
            sink += r.king_shield.to_bits();
        }
    }
    let rung2_core_ns = t.elapsed().as_nanos() as f64 / (ITERS as f64 * positions.len() as f64);

    println!(
        "# CVS speed bench (depth {depth}, suite {} positions)\n",
        positions.len()
    );
    println!("## Extraction throughput");
    println!("| Path | ns/call | calls/sec | vs rung2 |");
    println!("|---|---:|---:|---:|");
    println!(
        "| rung2 (baseline)      | {:.0} | {:.2}M | 1.00x |",
        rung2_ns,
        1e3 / rung2_ns
    );
    println!(
        "| rung2_core (cheap)    | {:.0} | {:.2}M | {:.2}x |",
        rung2_core_ns,
        1e3 / rung2_core_ns,
        rung2_core_ns / rung2_ns
    );
    println!(
        "| CVS features (Full)   | {:.0} | {:.2}M | {:.2}x |",
        cvs_ns,
        1e3 / cvs_ns,
        cvs_ns / rung2_ns
    );
    println!(
        "| CVS features (Core)   | {:.0} | {:.2}M | {:.2}x |",
        cvs_core_ns,
        1e3 / cvs_core_ns,
        cvs_core_ns / rung2_ns
    );

    // --- 3 & 4: search NPS, CVS trace off vs full vs core ---
    println!("\n## Search NPS (depth {depth})");
    println!("| Position | trace OFF nps | trace FULL nps | slowdown FULL | trace CORE nps | slowdown CORE |");
    println!("|---|---:|---:|---:|---:|---:|");
    let mut tot_off_nodes = 0u64;
    let mut tot_off_ms = 0u64;
    let mut tot_on_nodes = 0u64;
    let mut tot_on_ms = 0u64;
    let mut tot_core_nodes = 0u64;
    let mut tot_core_ms = 0u64;
    for (name, fen) in SUITE {
        let run = |trace: bool, core: bool| {
            let mut pos = Position::from_fen(fen).unwrap();
            let mut s = Searcher::new(Default::default(), None);
            let r = s.search(
                &mut pos,
                SearchOptions {
                    depth,
                    cvs_trace: trace,
                    cvs_core_trace: core,
                    ..Default::default()
                },
            );
            (
                r.telemetry.nodes,
                r.telemetry.elapsed_ms.max(1),
                r.telemetry.cvs_trace_features,
            )
        };
        let (off_n, off_ms, _) = run(false, false);
        let (on_n, on_ms, _) = run(true, false);
        let (core_n, core_ms, _) = run(false, true);
        
        let off_nps = off_n * 1000 / off_ms;
        let on_nps = on_n * 1000 / on_ms;
        let core_nps = core_n * 1000 / core_ms;
        
        tot_off_nodes += off_n;
        tot_off_ms += off_ms;
        tot_on_nodes += on_n;
        tot_on_ms += on_ms;
        tot_core_nodes += core_n;
        tot_core_ms += core_ms;
        
        println!(
            "| {name} | {} | {} | {:.2}x | {} | {:.2}x |",
            off_nps,
            on_nps,
            on_nps as f64 / off_nps.max(1) as f64,
            core_nps,
            core_nps as f64 / off_nps.max(1) as f64
        );
    }
    let off_nps = tot_off_nodes * 1000 / tot_off_ms.max(1);
    let on_nps = tot_on_nodes * 1000 / tot_on_ms.max(1);
    let core_nps = tot_core_nodes * 1000 / tot_core_ms.max(1);
    println!(
        "| **TOTAL** | **{}** | **{}** | **{:.2}x** | **{}** | **{:.2}x** |",
        off_nps,
        on_nps,
        on_nps as f64 / off_nps.max(1) as f64,
        core_nps,
        core_nps as f64 / off_nps.max(1) as f64
    );
    println!("registry_hash={:016x}", cvs_bitboard_core::eval::cvs_features::registry_hash());
    println!("core_registry_hash={:016x}", cvs_bitboard_core::eval::cvs_features::core_registry_hash());
    println!("\nsink={sink}");
}
