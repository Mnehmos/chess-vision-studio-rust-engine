//! Tests for the heterogeneous / specialist-lane SMP design
//! (CVS_HETEROGENEOUS_SMP.md).
//!
//! Two tiers:
//!   1. FOUNDATION invariants — testable today against the heterogeneous
//!      mechanic (`cvs_helpers`). These MUST hold or the lane design is unsafe
//!      to build on. They run and pass now.
//!   2. LANE-DESIGN tests — the real verification of each specialist lane and
//!      the Channel-A safety property. They need code that isn't built yet
//!      (ordering lanes, eval-kind TT tags, per-lane telemetry), so they are
//!      `#[ignore]`d with the exact assertion documented. Each becomes live as
//!      its feature lands.
use cvs_bitboard_core::movegen::generate_legal;
use cvs_bitboard_core::search::{SearchOptions, Searcher};
use cvs_bitboard_core::Position;

fn opts(depth: u32, threads: usize, cvs_helpers: usize) -> SearchOptions {
    SearchOptions { depth, threads, cvs_helpers, ..Default::default() }
}

// ─────────────────────────── Tier 1: foundation ───────────────────────────

/// Tactical correctness must survive ANY helper configuration — a specialist
/// lane that broke mate detection would be worse than useless. Mate-in-1 must
/// be found with a full heterogeneous helper fan-out.
#[test]
fn heterogeneous_config_still_finds_mate() {
    let fen = "r1bqkb1r/pppp1ppp/2n2n2/4p2Q/2B1P3/8/PPPP1PPP/RNB1K1NR w KQkq - 4 4";
    let mut pos = Position::from_fen(fen).unwrap();
    let mut s = Searcher::new(Default::default(), None);
    let r = s.search(&mut pos, opts(4, 4, 3)); // 3 "scout" helpers
    assert_eq!(r.mate, Some(1), "heterogeneous fan-out must not break tactics");
}

/// SAFETY: with no net loaded, `cvs_helpers > 0` must be inert — the het path
/// requires a net, so this must be byte-identical to homogeneous. Guards
/// against a lane config accidentally perturbing the default engine.
#[test]
fn cvs_helpers_without_net_is_homogeneous() {
    let fen = "r3k2r/p1ppqpb1/bn2pnp1/3PN3/1p2P3/2N2Q1p/PPPBBPPP/R3K2R w KQkq - 0 1";
    let homo = {
        let mut p = Position::from_fen(fen).unwrap();
        Searcher::new(Default::default(), None).search(&mut p, opts(6, 4, 0))
    };
    let het = {
        let mut p = Position::from_fen(fen).unwrap();
        Searcher::new(Default::default(), None).search(&mut p, opts(6, 4, 3))
    };
    // No net → no eval diversity → same depth-6 result (SMP score noise bounded).
    assert_eq!(homo.best_move, het.best_move, "no-net het must match homogeneous");
    assert!((homo.score_cp - het.score_cp).abs() <= 40);
}

/// Whatever the helper roster, the authoritative move must be legal.
#[test]
fn every_lane_config_returns_legal_move() {
    let fen = "r1b3nr/1pp1bkpp/p1n5/1q3p2/3P4/B1PNQ1P1/P4PBP/RN2R1K1 b - - 3 19";
    for helpers in [0, 1, 2, 3] {
        let mut pos = Position::from_fen(fen).unwrap();
        let r = Searcher::new(Default::default(), None).search(&mut pos, opts(6, 4, helpers));
        let legal = generate_legal(&mut Position::from_fen(fen).unwrap());
        assert!(legal.contains(&r.best_move.unwrap()), "illegal move at helpers={helpers}");
    }
}

/// A timed heterogeneous search must respect the clock (specialist helpers see
/// the same shared stop flag).
#[test]
fn timed_heterogeneous_search_terminates() {
    let mut pos = Position::startpos();
    let started = std::time::Instant::now();
    let r = Searcher::new(Default::default(), None).search(
        &mut pos,
        SearchOptions { depth: 30, max_time_ms: Some(400), threads: 4, cvs_helpers: 3, ..Default::default() },
    );
    assert!(started.elapsed().as_millis() < 2500);
    assert!(r.best_move.is_some());
}

// ─────────────────── Tier 2: lane-design tests (pending impl) ───────────────────

/// CHANNEL-A SAFETY INVARIANT — the keystone test of the whole design.
/// With lanes restricted to ordering-only (foreign-eval TT entries used as
/// move hints, never as score/bound cutoffs), the search RESULT (score, and on
/// a fixed seed the PV) must be IDENTICAL to single-thread — only faster.
/// Ordering changes which move is searched first, never the alpha-beta value.
/// Requires: eval-kind TT tags + a read path that drops foreign bounds.
#[test]
#[ignore = "needs eval-kind TT tagging (Channel-A read path) — Level 2 prerequisite"]
fn channel_a_ordering_only_preserves_score() {
    // let fen = "...quiet midgame...";
    // let single = search(threads=1);
    // let laned  = search(threads=4, lanes=[king,see,tactics], tt_foreign=OrderingOnly);
    // assert_eq!(single.score_cp, laned.score_cp);   // bound identity = correctness
    // assert!(laned.telemetry.nodes_walltime < single's at equal depth);
}

/// KING-SAFETY LANE behaviour: on a king-danger position, the king-safety scout
/// must ORDER a defensive/king-saving move into the first slots it writes to
/// the TT (even if the fast eval wouldn't rank it first). Verifiable via the
/// lane's first-written TT move on the 4fxkLVBb pre-Bd6 FEN, where SF's choice
/// (c8d7) defends rather than the materially-tempting e7d6.
#[test]
#[ignore = "needs KingSafetyScout ordering lane (Level 1)"]
fn king_safety_lane_orders_defensive_move_first() {
    // let pos = "r1b3nr/1pp1bkpp/p1n5/1q3p2/3P4/B1PNQ1P1/P4PBP/RN2R1K1 b - - 3 19";
    // let tt_move = run_lane(KingSafety, pos, depth=6).first_root_tt_move();
    // assert!(reduces_king_danger(pos, tt_move));   // defensive bias, not material
}

/// SEE/HANGING LANE: on a position with a free hanging piece, the SEE scout's
/// first TT move must be the clean SEE-winning capture (or the rescue), ahead
/// of quiet moves the fast eval might prefer.
#[test]
#[ignore = "needs SEE/HangingScout ordering lane (Level 1)"]
fn see_lane_prioritizes_clean_material() {
    // let pos = "...position with a SEE>0 capture...";
    // let tt_move = run_lane(See, pos, depth=4).first_root_tt_move();
    // assert!(see(pos, tt_move) > 0);
}

/// ANTI-VIBES TRANSFER METRIC — the soul of the design. A lane earns its core
/// only if its TT suggestions improve the MAIN thread's final move. Per lane we
/// must record foreign_moves_tried / became_pv / caused_cutoff. This test
/// asserts the telemetry exists and that on the danger suite a king-safety lane
/// has became_pv > 0 (it actually influenced the authoritative result).
#[test]
#[ignore = "needs per-lane transfer telemetry + KingSafetyScout"]
fn king_lane_transfer_is_measured_and_positive_on_danger_suite() {
    // let tel = run(main=fast, lanes=[king], suite=danger_suite_epd).lane_telemetry(KingSafety);
    // assert!(tel.foreign_moves_became_pv > 0, "lane that never reaches PV is noise — kill it");
    // assert!(tel.danger_suite_regressions == 0);
}

/// Each specialist lane must NOT break tactical correctness in isolation —
/// run each lane solo (main + one lane) and confirm mate-in-1 still found.
#[test]
#[ignore = "needs lane selection flag (--helper-lanes)"]
fn each_lane_solo_preserves_mate() {
    // for lane in [KingSafety, See, DefenderRemoval, Tactics, QuietDefense, PawnEndgame] {
    //     assert_eq!(run(main=fast, lanes=[lane], mate_in_1_fen).mate, Some(1));
    // }
}

/// PAWN/ENDGAME LANE gating: it should only engage in low-material positions
/// (the design enables it by phase). In a queen-heavy middlegame it must be a
/// no-op (write nothing / behave as a fast helper).
#[test]
#[ignore = "needs PawnEndgameScout + phase gate"]
fn pawn_lane_inert_in_high_material() {
    // let tel = run(main=fast, lanes=[pawn], kiwipete).lane_telemetry(PawnEndgame);
    // assert_eq!(tel.tt_entries_written, 0, "pawn lane must idle in the middlegame");
}
