//! Perft / nodes-per-second benchmark + correctness report.
//!
//!   cargo run --release --bin perft                 # full report (validate + bench)
//!   cargo run --release --bin perft -- "<fen>" 5     # perft divide for one position
use cvs_bitboard_core::perft::{perft, perft_divide};
use cvs_bitboard_core::Position;
use std::time::Instant;

const SUITE: &[(&str, &str, &[(u32, u64)])] = &[
    (
        "startpos",
        "rnbqkbnr/pppppppp/8/8/8/8/PPPPPPPP/RNBQKBNR w KQkq - 0 1",
        &[(1, 20), (2, 400), (3, 8902), (4, 197281), (5, 4865609)],
    ),
    (
        "kiwipete",
        "r3k2r/p1ppqpb1/bn2pnp1/3PN3/1p2P3/2N2Q1p/PPPBBPPP/R3K2R w KQkq - 0 1",
        &[(1, 48), (2, 2039), (3, 97862), (4, 4085603)],
    ),
    (
        "position3",
        "8/2p5/3p4/KP5r/1R3p1k/8/4P1P1/8 w - - 0 1",
        &[(1, 14), (2, 191), (3, 2812), (4, 43238), (5, 674624)],
    ),
    (
        "position4",
        "r3k2r/Pppp1ppp/1b3nbN/nP6/BBP1P3/q4N2/Pp1P2PP/R2Q1RK1 w kq - 0 1",
        &[(1, 6), (2, 264), (3, 9467), (4, 422333)],
    ),
    (
        "position5",
        "rnbq1k1r/pp1Pbppp/2p5/8/2B5/8/PPP1NnPP/RNBQK2R w KQ - 1 8",
        &[(1, 44), (2, 1486), (3, 62379), (4, 2103487)],
    ),
    (
        "position6",
        "r4rk1/1pp1qppp/p1np1n2/2b1p1B1/2B1P1b1/P1NP1N2/1PP1QPPP/R4RK1 w - - 0 10",
        &[(1, 46), (2, 2079), (3, 89890), (4, 3894594)],
    ),
];

fn main() {
    let args: Vec<String> = std::env::args().collect();
    if args.len() >= 3 {
        // Divide mode: <fen> <depth>
        let fen = &args[1];
        let depth: u32 = args[2].parse().expect("depth");
        let mut pos = Position::from_fen(fen).expect("valid FEN");
        let t = Instant::now();
        let div = perft_divide(&mut pos, depth);
        let total: u64 = div.iter().map(|(_, n)| n).sum();
        for (mv, n) in &div {
            println!("{}: {}", mv.to_uci(), n);
        }
        let secs = t.elapsed().as_secs_f64();
        println!("\nnodes {total}  time {secs:.3}s  nps {:.0}", total as f64 / secs.max(1e-9));
        return;
    }

    println!("CVS Bitboard Core v0 — perft report\n");
    println!("| Position | Depth | Nodes | Expected | OK | Time(s) | Nodes/s |");
    println!("|---|---:|---:|---:|:--:|---:|---:|");
    let mut all_ok = true;
    for (name, fen, expected) in SUITE {
        let mut pos = Position::from_fen(fen).expect("valid FEN");
        for &(depth, exp) in *expected {
            let t = Instant::now();
            let nodes = perft(&mut pos, depth);
            let secs = t.elapsed().as_secs_f64();
            let ok = nodes == exp;
            all_ok &= ok;
            let nps = nodes as f64 / secs.max(1e-9);
            println!(
                "| {name} | {depth} | {nodes} | {exp} | {} | {secs:.3} | {nps:.0} |",
                if ok { "✓" } else { "✗" }
            );
        }
    }

    // Focused nodes/sec benchmark at depth 4 and 5 (startpos), release-meaningful.
    println!("\n## nodes/sec benchmark (startpos)");
    for depth in [4u32, 5u32] {
        let mut pos = Position::startpos();
        let t = Instant::now();
        let nodes = perft(&mut pos, depth);
        let secs = t.elapsed().as_secs_f64();
        println!(
            "depth {depth}: {nodes} nodes in {secs:.3}s = {:.0} nodes/s",
            nodes as f64 / secs.max(1e-9)
        );
    }

    println!("\n{}", if all_ok { "ALL PERFT CHECKS PASSED" } else { "PERFT MISMATCH — see ✗ rows" });
    if !all_ok {
        std::process::exit(1);
    }
}
