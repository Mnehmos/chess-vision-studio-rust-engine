//! R3 search acceptance suite, layer by layer:
//!   R3.1 plain negamax/αβ — mate-in-1, obvious capture, sign, mate scoring
//!   R3.2 capture quiescence — no horizon garbage on defended pawns
//!   R3.3 forcing quiet checks — the d4 forensic regression (mixed weights)
//!   R3.5 TT — on/off gives the same move + score on a fixed battery
use cvs_bitboard_core::eval::{Rung2Weights, ValueWeights};
use cvs_bitboard_core::eval::weights::MaterialWeights;
use cvs_bitboard_core::movegen::generate_legal;
use cvs_bitboard_core::search::{SearchOptions, Searcher, MATE_SCORE};
use cvs_bitboard_core::Position;

fn pos(fen: &str) -> Position {
    Position::from_fen(fen).unwrap()
}

fn opts(depth: u32, quiet_checks: bool, use_tt: bool) -> SearchOptions {
    SearchOptions { depth, max_time_ms: None, quiet_checks, use_tt, danger_extension: false }
}

/// The trained Rung-2 mixed weights — same fixture as the TS search-boundary suite.
fn mixed() -> (ValueWeights, Rung2Weights) {
    let base = ValueWeights {
        material: MaterialWeights {
            p: 0.9771941394330501,
            n: 0.9472182607514921,
            b: 0.9749786048046231,
            r: 0.9692686197283779,
            q: 0.9808573066642514,
        },
        pst_scale: 1.0132826900617409,
        bishop_pair: 27.855206547893538,
        tempo: 12.353636757965402,
    };
    let rung2 = Rung2Weights {
        mobility_knight: 0.3446948773300695,
        mobility_bishop: 2.4737715292977267,
        mobility_rook: 1.228760832057703,
        mobility_queen: 0.5667502267756924,
        king_shield: 1.1851233890189907,
        king_zone_pressure: 4.907791717892074,
        king_open_file: 4.287137678536876,
        passed_pawn_mg: 0.8814455206517932,
        passed_pawn_eg: 1.1306107911887235,
        connected_passed_pawn: 3.2949975972383156,
        rook_open_file: 2.9742065689960477,
        rook_semi_open_file: 10.791224386274036,
        rook_seventh: 3.182875456707168,
        doubled_pawn: 2.8046149341674074,
        isolated_pawn: -0.91588157836913,
        bishop_pair_mg: -1.6363672647332876,
        bishop_pair_eg: -0.5084261873731701,
        hanging_piece: 11.6631476916599,
    };
    (base, rung2)
}

// ---- R3.1: plain negamax/alpha-beta (no quiescence layers beyond captures, no TT) ----

#[test]
fn r31_finds_mate_in_one_white() {
    let mut p = pos("6k1/5ppp/8/8/8/8/8/R6K w - - 0 1");
    let mut s = Searcher::new(ValueWeights::default(), None);
    let r = s.search(&mut p, opts(2, false, false));
    assert_eq!(r.best_move.unwrap().to_uci(), "a1a8");
    assert_eq!(r.mate, Some(1));
    assert!(r.score_cp > MATE_SCORE - 10);
}

#[test]
fn r31_finds_mate_in_one_black() {
    let mut p = pos("r6k/8/8/8/8/8/5PPP/6K1 b - - 0 1");
    let mut s = Searcher::new(ValueWeights::default(), None);
    let r = s.search(&mut p, opts(2, false, false));
    assert_eq!(r.best_move.unwrap().to_uci(), "a8a1");
    assert_eq!(r.mate, Some(1));
}

#[test]
fn r31_takes_the_hanging_queen() {
    // White Qh1 vs undefended Black Qh5 on an open file.
    let mut p = pos("4k3/8/8/7q/8/8/8/4K2Q w - - 0 1");
    let mut s = Searcher::new(ValueWeights::default(), None);
    let r = s.search(&mut p, opts(3, false, false));
    assert_eq!(r.best_move.unwrap().to_uci(), "h1h5");
    assert!(r.score_cp > 700);
}

#[test]
fn r31_stalemate_root_scores_zero_with_no_move() {
    let mut p = pos("7k/5Q2/6K1/8/8/8/8/8 b - - 0 1");
    let mut s = Searcher::new(ValueWeights::default(), None);
    let r = s.search(&mut p, opts(3, false, false));
    assert!(r.best_move.is_none());
    assert_eq!(r.score_cp, 0);
}

#[test]
fn r31_best_move_is_always_legal() {
    for fen in [
        "r3k2r/p1ppqpb1/bn2pnp1/3PN3/1p2P3/2N2Q1p/PPPBBPPP/R3K2R w KQkq - 0 1",
        "8/2p5/3p4/KP5r/1R3p1k/8/4P1P1/8 w - - 0 1",
        "rnbq1k1r/pp1Pbppp/2p5/8/2B5/8/PPP1NnPP/RNBQK2R w KQ - 1 8",
    ] {
        let mut p = pos(fen);
        let mut s = Searcher::new(ValueWeights::default(), None);
        let r = s.search(&mut p, opts(3, true, true));
        let best = r.best_move.expect("search must return a move");
        assert!(generate_legal(&mut p).contains(&best), "illegal best move on {fen}");
    }
}

// ---- R3.2: capture quiescence ----

#[test]
fn r32_does_not_grab_the_defended_pawn() {
    // Black d5 pawn is defended by c6; Qxd5 loses the queen to the recapture.
    // Without quiescence a depth-1 search would grab it; the qsearch must refuse.
    let mut p = pos("4k3/8/2p5/3p4/8/8/3Q4/4K3 w - - 0 1");
    let mut s = Searcher::new(ValueWeights::default(), None);
    let r = s.search(&mut p, opts(1, false, false));
    assert_ne!(r.best_move.unwrap().to_uci(), "d2d5");
}

#[test]
fn r32_quiescence_telemetry_populates() {
    let mut p = pos("r3k2r/p1ppqpb1/bn2pnp1/3PN3/1p2P3/2N2Q1p/PPPBBPPP/R3K2R w KQkq - 0 1");
    let mut s = Searcher::new(ValueWeights::default(), None);
    let r = s.search(&mut p, opts(3, true, true));
    assert!(r.telemetry.q_nodes > 0);
    assert!(r.telemetry.q_capture_nodes > 0);
    assert!(r.telemetry.max_q_depth < 40, "quiescence must stay bounded");
}

// ---- R3.3: forcing quiet-check extensions (the d4 lesson) ----

#[test]
fn r33_d4_forensic_avoids_the_quiet_refuted_bf7() {
    // Holdout #549: Bf7 (b3f7) is a 2.18-pawn blunder whose refutation is QUIET.
    // With the trained mixed weights at depth 4, capture-only quiescence fell for
    // it in TS; the forcing quiet-check extension must avoid it.
    let (base, rung2) = mixed();
    let mut p = pos("5r2/pp5R/1kp3p1/6b1/4P1b1/1BNP2P1/PPP4P/1K6 w - - 1 22");
    let mut s = Searcher::new(base, Some(rung2));
    let r = s.search(&mut p, opts(4, true, true));
    assert_ne!(r.best_move.unwrap().to_uci(), "b3f7", "quiet-refuted blunder must be avoided");
    assert!(r.telemetry.quiet_check_extensions > 0, "quiet-check extension should fire here");
}

// ---- R3.5: transposition table ----

#[test]
fn r35_tt_preserves_move_and_score() {
    // The TT is a cache, not a behavior change: same best move + score on a battery.
    let battery = [
        ("rnbqkbnr/pppppppp/8/8/8/8/PPPPPPPP/RNBQKBNR w KQkq - 0 1", 3),
        ("r3k2r/p1ppqpb1/bn2pnp1/3PN3/1p2P3/2N2Q1p/PPPBBPPP/R3K2R w KQkq - 0 1", 2),
        ("5r2/pp5R/1kp3p1/6b1/4P1b1/1BNP2P1/PPP4P/1K6 w - - 1 22", 3),
        ("4r1k1/1p3pp1/p1p3rp/P1Qnq3/1PB5/4P3/5PPP/3R1RK1 b - - 5 27", 2),
    ];
    for (fen, depth) in battery {
        let mut p1 = pos(fen);
        let mut s1 = Searcher::new(ValueWeights::default(), None);
        let with_tt = s1.search(&mut p1, opts(depth, true, true));
        let mut p2 = pos(fen);
        let mut s2 = Searcher::new(ValueWeights::default(), None);
        let without_tt = s2.search(&mut p2, opts(depth, true, false));
        assert_eq!(
            with_tt.best_move.map(|m| m.to_uci()),
            without_tt.best_move.map(|m| m.to_uci()),
            "TT changed the best move on {fen}"
        );
        assert_eq!(with_tt.score_cp, without_tt.score_cp, "TT changed the score on {fen}");
    }
}

#[test]
fn r35_pv_is_a_legal_line() {
    let mut p = pos("r3k2r/p1ppqpb1/bn2pnp1/3PN3/1p2P3/2N2Q1p/PPPBBPPP/R3K2R w KQkq - 0 1");
    let mut s = Searcher::new(ValueWeights::default(), None);
    let r = s.search(&mut p, opts(3, true, true));
    assert!(!r.pv.is_empty());
    // Replay the PV: every move must be legal in sequence.
    let mut replay = pos("r3k2r/p1ppqpb1/bn2pnp1/3PN3/1p2P3/2N2Q1p/PPPBBPPP/R3K2R w KQkq - 0 1");
    for mv in &r.pv {
        assert!(generate_legal(&mut replay).contains(mv), "PV move {} not legal", mv.to_uci());
        replay.make(*mv);
    }
}

#[test]
fn zobrist_hash_is_make_unmake_stable() {
    let mut p = pos("r3k2r/p1ppqpb1/bn2pnp1/3PN3/1p2P3/2N2Q1p/PPPBBPPP/R3K2R w KQkq - 0 1");
    let h0 = p.hash;
    for mv in generate_legal(&mut p) {
        p.make(mv);
        let inner = generate_legal(&mut p);
        if let Some(&m2) = inner.first() {
            p.make(m2);
            p.unmake();
        }
        p.unmake();
        assert_eq!(p.hash, h0, "hash drift after make/unmake of {}", mv.to_uci());
    }
}
