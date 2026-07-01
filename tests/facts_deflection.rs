//! Adversarial FEN battery for the DEFLECTION / DISTRACTION detector (ChessTempo).
//!
//! A MOVE detector, the DISJOINT complement of attacking_the_defender: a move that does
//! NOT capture the enemy defender D yet creates a FORCING threat evicting D from its post,
//! where D is NOT profitably capturable in place (best_see_capture(after_us, d_sq) <= 0 —
//! the exact opposite of attack_defender's d_in_place > 0 gate). D is the SOLE guard of one
//! or more enemy charges P. Against EVERY legal enemy reply we still win material (win the
//! standing/relocated defender, or a charge the eviction abandons). Because both detectors
//! share attack_defender_charges + attack_defender_worst_case and differ only by that single
//! flipped gate, any attack_defender positive is a guaranteed deflection NEGATIVE (DN2) and
//! vice-versa — the AP/AN battery doubles as the disjointness oracle.
//!
//! material_gain uses SEE_VALUE (knight 320, bishop 330, rook 500) — the best_see_capture
//! scale, the least-bad outcome over the enemy's best reply. The load-bearing negative is
//! DN1: a naive D-escape-only detector fires it; the sound all-replies detector must not
//! (the guarded rook counter-captures our rook, saving everything).

use cvs_bitboard_core::facts::motifs::deflection_opportunities;
use cvs_bitboard_core::facts::{DeflectionOpportunity, FactCollection, PieceType};
use cvs_bitboard_core::Position;

fn dfl(fen: &str) -> Vec<DeflectionOpportunity> {
    let pos = Position::from_fen(fen).unwrap();
    match deflection_opportunities(&pos) {
        FactCollection::Computed { items } => items,
        other => panic!("deflection should be computed, got {other:?}"),
    }
}

fn by_move<'a>(it: &'a [DeflectionOpportunity], uci: &str) -> Option<&'a DeflectionOpportunity> {
    it.iter().find(|o| o.move_uci == uci)
}

fn target_squares(o: &DeflectionOpportunity) -> Vec<&str> {
    o.targets.iter().map(|t| t.square.as_str()).collect()
}

// ── positives (must fire) ─────────────────────────────────────────────────────

#[test]
fn a_king_move_overwhelms_the_sole_guard() {
    // DP1 (king mover): Kf2 adds a second attacker on Ne2, whose only defender is Re5.
    // The knight cannot be held; against every reply we win it (320). Re5 itself is not
    // capturable in place (nothing attacks e5), so this is deflection, not attack_defender.
    let items = dfl("6k1/8/8/4r3/8/8/4n3/4R1K1 w - - 0 1");
    assert_eq!(items.len(), 1, "exactly one deflection: {items:?}");
    let o = by_move(&items, "g1f2").expect("Kf2 deflects the rook-guard Re5");
    assert_eq!(o.kind, "deflection");
    assert_eq!(o.validator, "deflection_validation");
    assert_eq!(o.mover.piece_type, PieceType::King);
    assert_eq!(o.mover.square, "f2");
    assert_eq!(o.distracted_defender.piece_type, PieceType::Rook);
    assert_eq!(o.distracted_defender.square, "e5");
    assert_eq!(target_squares(o), vec!["e2"]);
    assert!(!o.gives_check);
    assert_eq!(o.material_gain, 320);
}

#[test]
fn a_knight_develops_to_overload_the_back_rook() {
    // DP2 (knight mover, primary positive): Nb1-c3 adds a second attacker on Bd5, whose only
    // defender is the back-rank Rd8. The bishop falls (330). Rd8 is unreachable in place.
    let items = dfl("3r2k1/8/8/3b4/8/8/8/1N1R2K1 w - - 0 1");
    assert_eq!(items.len(), 1, "exactly one deflection: {items:?}");
    let o = by_move(&items, "b1c3").expect("Nc3 deflects the rook-guard Rd8");
    assert_eq!(o.kind, "deflection");
    assert_eq!(o.mover.piece_type, PieceType::Knight);
    assert_eq!(o.mover.square, "c3");
    assert_eq!(o.distracted_defender.piece_type, PieceType::Rook);
    assert_eq!(o.distracted_defender.square, "d8");
    assert_eq!(target_squares(o), vec!["d5"]);
    assert!(!o.gives_check);
    assert_eq!(o.material_gain, 330);
}

#[test]
fn a_pawn_push_can_deflect_the_guard() {
    // DP3 (pawn mover, proof of non-slider / quiet generality): e3-e4 adds a second attacker
    // on Nd5, sole-guarded by Rd8. The knight is lost (320); the pushed pawn does not hang
    // (Nd5 does not attack e4).
    let items = dfl("3r2k1/8/8/3n4/8/4P3/8/3R2K1 w - - 0 1");
    assert_eq!(items.len(), 1, "exactly one deflection: {items:?}");
    let o = by_move(&items, "e3e4").expect("e4 deflects the rook-guard Rd8");
    assert_eq!(o.mover.piece_type, PieceType::Pawn);
    assert_eq!(o.mover.square, "e4");
    assert_eq!(o.distracted_defender.piece_type, PieceType::Rook);
    assert_eq!(o.distracted_defender.square, "d8");
    assert_eq!(target_squares(o), vec!["d5"]);
    assert_eq!(o.material_gain, 320);
}

// ── adversarial negatives (must produce ZERO ops) ─────────────────────────────

#[test]
fn rejects_when_the_charge_counter_captures_our_attacker() {
    // DN1 (LOAD-BEARING): the guarded rook replies Rxd1, grabbing our undefended rook and
    // saving everything. The all-replies worst-case returns None — the exact refutation the
    // detector comments warn about.
    assert!(
        dfl("3r2k1/8/4n3/8/8/8/8/3R1BK1 w - - 0 1").is_empty(),
        "the enemy counter-capture of our attacker refutes the eviction"
    );
}

#[test]
fn rejects_when_the_defender_is_capturable_in_place() {
    // DN2 (DISJOINTNESS ORACLE): AP1 for attacking_the_defender — ...Qc2 attacks the
    // UNDEFENDED Nb3, so best_see_capture(after_us, b3) > 0 and the complement gate rejects
    // it. This position fires attacking_the_defender and must be SILENT here (no double-emit).
    assert!(
        dfl("8/2q1k3/8/8/8/1N6/3B2K1/8 b - - 0 1").is_empty(),
        "a defender winnable in place belongs to attacking_the_defender, not deflection"
    );
}

#[test]
fn rejects_when_our_mover_hangs() {
    // DN3: 1.Bc4 hangs to ...bxc4 — our mover is simply lost (guard A).
    assert!(dfl("3r2k1/8/4n3/1p6/8/8/8/3R1BK1 w - - 0 1").is_empty());
}

#[test]
fn rejects_when_the_charge_is_already_winnable() {
    // DN4: doubled rooks already win the sole-defended piece before any move → causality
    // guard: attack_defender_charges returns empty.
    assert!(dfl("3r2k1/8/4n3/8/8/8/3R4/3R1BK1 w - - 0 1").is_empty());
}

#[test]
fn no_ops_in_the_opening_position() {
    // DN5: no sole-defended charges.
    assert!(dfl("rnbqkbnr/pppppppp/8/8/8/8/PPPPPPPP/RNBQKBNR w KQkq - 0 1").is_empty());
}

// ── invariant ─────────────────────────────────────────────────────────────────

#[test]
fn enumeration_does_not_mutate_the_position() {
    let fen = "3r2k1/8/8/3b4/8/8/8/1N1R2K1 w - - 0 1";
    let pos = Position::from_fen(fen).unwrap();
    let _ = deflection_opportunities(&pos);
    assert_eq!(pos.to_fen(), fen, "deflection enumeration must not mutate the board");
}
