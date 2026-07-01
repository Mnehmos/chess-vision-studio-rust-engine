//! Adversarial FEN battery for the LURING-THE-DEFENDER (DECOY) detector.
//!
//! A MOVE detector in the remove-the-defender family, occupying the sac-to-decoy slice that
//! deflection / attacking_the_defender / fork / double_attack all REJECT: the mover lands on
//! a square `s` where it IS profitably capturable (an offered SACRIFICE), and the enemy's
//! forced recapture is by a piece D that is the SOLE guard of a charge P. The defining gate is
//! `forker_capturable_for_gain(after, s) == TRUE` — the exact case those four detectors bail on
//! — so any deflection/attack_defender positive is a guaranteed lure NEGATIVE (LN1) and vice
//! versa; the detector is provably disjoint from them and never double-emits. D must additionally
//! BE the forced (cheapest) recapturer of `s`, else the enemy takes `s` with a throwaway and keeps
//! guarding P (LN3). Against EVERY legal enemy reply we still net material AFTER paying the sac:
//! attack_defender_worst_case debits the sacrificed decoy exactly once (LN2/LN5).
//!
//! material_gain uses the best_see_capture / SEE scale — the least-bad outcome over the enemy's
//! best reply, already debited for the decoy.

use cvs_bitboard_core::facts::motifs::lure_defender_opportunities;
use cvs_bitboard_core::facts::{FactCollection, LureDefenderOpportunity, PieceType};
use cvs_bitboard_core::Position;

fn lure(fen: &str) -> Vec<LureDefenderOpportunity> {
    let pos = Position::from_fen(fen).unwrap();
    match lure_defender_opportunities(&pos) {
        FactCollection::Computed { items } => items,
        other => panic!("lure should be computed, got {other:?}"),
    }
}

fn by_move<'a>(it: &'a [LureDefenderOpportunity], uci: &str) -> Option<&'a LureDefenderOpportunity> {
    it.iter().find(|o| o.move_uci == uci)
}

fn target_squares(o: &LureDefenderOpportunity) -> Vec<&str> {
    o.targets.iter().map(|t| t.square.as_str()).collect()
}

// ── positives (must fire exactly one op) ──────────────────────────────────────

#[test]
fn lp1_classic_rook_defender_decoy() {
    // LP1: Ba4-c2 lands the bishop en prise on c2 where ONLY the black rook f2 can recapture
    // (along rank 2). Rf2 is the sole guard of Rf5 (along the f-file). After Rxc2 the f-rook
    // leaves the file and Qd5xf5 wins the rook. Qd5 is defended by Kd6, so the charge Rf5 cannot
    // counter-capture the queen. Net +200 (rook won, bishop sac debited).
    let items = lure("8/7k/3KR3/3Q1r2/B7/8/5r2/8 w - - 0 1");
    assert_eq!(items.len(), 1, "exactly one lure: {items:?}");
    let o = by_move(&items, "a4c2").expect("Bc2 decoys the rook-guard Rf2");
    assert_eq!(o.kind, "luring_the_defender");
    assert_eq!(o.validator, "lure_defender_validation");
    assert_eq!(o.mover.piece_type, PieceType::Bishop);
    assert_eq!(o.mover.square, "c2");
    assert_eq!(o.lured_defender.piece_type, PieceType::Rook);
    assert_eq!(o.lured_defender.square, "f2");
    assert_eq!(target_squares(o), vec!["f5"]);
    assert!(!o.gives_check);
    assert_eq!(o.material_gain, 200);
}

#[test]
fn lp2_pawn_recapturer_is_the_lured_defender() {
    // LP2: Bc8-f5 lands en prise on f5 whose sole legal recapturer is the g6 pawn (gxf5). The
    // g6 pawn is the sole guard of the h5 bishop (a black pawn on g6 defends f5 and h5). After
    // the forced gxf5 the h5 bishop hangs and Qg2xh5 wins it. Proves D-as-pawn is admitted and
    // that cheapest_legal_capturer_from picks the pawn.
    let items = lure("2B1R3/8/6p1/k6b/7K/8/6Q1/8 w - - 0 1");
    assert_eq!(items.len(), 1, "exactly one lure: {items:?}");
    let o = by_move(&items, "c8f5").expect("Bf5 decoys the pawn-guard g6");
    assert_eq!(o.mover.piece_type, PieceType::Bishop);
    assert_eq!(o.mover.square, "f5");
    assert_eq!(o.lured_defender.piece_type, PieceType::Pawn);
    assert_eq!(o.lured_defender.square, "g6");
    assert_eq!(target_squares(o), vec!["h5"]);
    assert!(!o.gives_check);
    assert!(o.material_gain > 0);
}

#[test]
fn lp3_queen_decoy_is_forced() {
    // LP3: Bd1-a4 lands en prise on a4 where the black QUEEN b5 is the unique legal capturer
    // (Qxa4). Qb5 is the sole guard of Rd5 (along rank 5). The forced Qxa4 abandons the rook and
    // Qg2xd5 wins it. Proves the min-cost-capturer identity when only one enemy piece can take s.
    let items = lure("4k2K/8/7R/1q1r4/8/8/6Q1/3B4 w - - 0 1");
    assert_eq!(items.len(), 1, "exactly one lure: {items:?}");
    let o = by_move(&items, "d1a4").expect("Ba4 decoys the queen-guard Qb5");
    assert_eq!(o.mover.piece_type, PieceType::Bishop);
    assert_eq!(o.mover.square, "a4");
    assert_eq!(o.lured_defender.piece_type, PieceType::Queen);
    assert_eq!(o.lured_defender.square, "b5");
    assert_eq!(target_squares(o), vec!["d5"]);
    assert!(!o.gives_check);
    assert_eq!(o.material_gain, 200);
}

// ── adversarial negatives (must produce ZERO ops) ─────────────────────────────

#[test]
fn ln1_disjointness_oracle_a_deflection_positive_is_silent() {
    // LN1 (DISJOINTNESS ORACLE): deflection DP2 — Nc3 is NOT capturable in place
    // (forker_capturable_for_gain == FALSE), so it belongs to `deflection`, not the sac-to-decoy
    // slice. The defining lure gate rejects it → SILENT here (no double-emission).
    assert!(
        lure("3r2k1/8/8/3b4/8/8/8/1N1R2K1 w - - 0 1").is_empty(),
        "a non-sacrificial deflection belongs to deflection, not luring-the-defender"
    );
}

#[test]
fn ln2_rejects_when_the_charge_counter_captures() {
    // LN2 (LOAD-BEARING): the LP1 shape but the queen d5 is UNDEFENDED (white king moved off d6),
    // so the charge Rf5 replies Rxd5 grabbing the queen and saving itself. The debit-aware
    // all-replies worst-case returns None — the counter-capturing refutation the spec warns about.
    assert!(
        lure("8/7k/4R3/3Q1r2/B7/8/5r2/1K6 w - - 0 1").is_empty(),
        "the charge's counter-capture of our attacker refutes the decoy"
    );
}

#[test]
fn ln3_rejects_when_a_cheaper_non_defender_can_recapture() {
    // LN3 (wrong recapturer — lure not forced): the LP1 shape but a black pawn on d3 also captures
    // c2 (dxc2), more cheaply than the rook f2. cheapest_legal_capturer_from picks the pawn, so
    // recapturer_from != f2 and the rook is not forced off the f-file. The enemy recaptures with
    // the throwaway pawn and keeps guarding Rf5 → must be SILENT.
    assert!(
        lure("8/7k/3KR3/3Q1r2/B7/3p4/5r2/8 w - - 0 1").is_empty(),
        "a cheaper non-defender recapturer means the guard is not forced off"
    );
}

#[test]
fn ln4_rejects_when_the_charge_is_already_winnable() {
    // LN4 (causality): doubled rooks already win the sole-defended knight before any move, so
    // attack_defender_charges returns empty (best_see_capture(pos, charge) > 0). (deflection DN4)
    assert!(
        lure("3r2k1/8/4n3/8/8/8/3R4/3R1BK1 w - - 0 1").is_empty(),
        "a charge already winnable is not caused by the decoy"
    );
}

#[test]
fn ln5_rejects_a_counter_capture_with_collateral() {
    // LN5 (fuzz-class collateral counter-capture): the a4-knight cannot land as a for-gain sac
    // that survives the enemy's debit-aware best reply; the enemy_take debit in the all-replies
    // worst-case rejects any bogus positive. (deflection DN6 shape.)
    let items = lure("r5k1/3bbrpp/p1qp4/3QpR2/N2BP3/2p5/PPP3PP/6K1 w - - 0 10");
    assert!(
        items.iter().all(|o| o.move_uci != "a4b6"),
        "a counter-capturing refutation with collateral must reject the decoy, got {items:?}"
    );
}

#[test]
fn ln6_no_ops_in_the_opening_position() {
    // No sole-defended charges reachable by a surviving sacrifice.
    assert!(lure("rnbqkbnr/pppppppp/8/8/8/8/PPPPPPPP/RNBQKBNR w KQkq - 0 1").is_empty());
}

// ── invariant ─────────────────────────────────────────────────────────────────

#[test]
fn enumeration_does_not_mutate_the_position() {
    let fen = "8/7k/3KR3/3Q1r2/B7/8/5r2/8 w - - 0 1";
    let pos = Position::from_fen(fen).unwrap();
    let _ = lure_defender_opportunities(&pos);
    assert_eq!(
        pos.to_fen(),
        fen,
        "lure enumeration must not mutate the board"
    );
}
