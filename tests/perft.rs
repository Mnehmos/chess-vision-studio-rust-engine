//! Perft correctness suite — the canonical positions (Chess Programming Wiki),
//! covering normal moves, captures, castling, en passant, promotions, and
//! check/evasion. Depths chosen to exercise every edge case while staying fast in
//! a debug build; the deep nodes/sec runs live in the `perft` benchmark binary.
use cvs_bitboard_core::{perft::perft, Position};

fn check(fen: &str, expected: &[(u32, u64)]) {
    let mut pos = Position::from_fen(fen).expect("valid FEN");
    for &(depth, nodes) in expected {
        assert_eq!(perft(&mut pos, depth), nodes, "perft({depth}) on {fen}");
    }
}

#[test]
fn perft_startpos() {
    check(
        "rnbqkbnr/pppppppp/8/8/8/8/PPPPPPPP/RNBQKBNR w KQkq - 0 1",
        &[(1, 20), (2, 400), (3, 8902), (4, 197281)],
    );
}

#[test]
fn perft_kiwipete() {
    // Castling, captures, pins, deep tactics.
    check(
        "r3k2r/p1ppqpb1/bn2pnp1/3PN3/1p2P3/2N2Q1p/PPPBBPPP/R3K2R w KQkq - 0 1",
        &[(1, 48), (2, 2039), (3, 97862)],
    );
}

#[test]
fn perft_position3_ep_and_checks() {
    // En passant, rook checks, king-and-pawn play.
    check(
        "8/2p5/3p4/KP5r/1R3p1k/8/4P1P1/8 w - - 0 1",
        &[(1, 14), (2, 191), (3, 2812), (4, 43238)],
    );
}

#[test]
fn perft_position4_castling_and_promotions() {
    // Promotions, castling rights, a side in check.
    check(
        "r3k2r/Pppp1ppp/1b3nbN/nP6/BBP1P3/q4N2/Pp1P2PP/R2Q1RK1 w kq - 0 1",
        &[(1, 6), (2, 264), (3, 9467)],
    );
}

#[test]
fn perft_position5_promotions() {
    check(
        "rnbq1k1r/pp1Pbppp/2p5/8/2B5/8/PPP1NnPP/RNBQK2R w KQ - 1 8",
        &[(1, 44), (2, 1486), (3, 62379)],
    );
}

#[test]
fn perft_position6() {
    check(
        "r4rk1/1pp1qppp/p1np1n2/2b1p1B1/2B1P1b1/P1NP1N2/1PP1QPPP/R4RK1 w - - 0 10",
        &[(1, 46), (2, 2079), (3, 89890)],
    );
}
