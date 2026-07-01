//! Adversarial FEN battery for the OVERLOADING detector (ChessTempo "Overloading").
//!
//! An enemy piece D is OVERLOADED when it is the sole/critical defender of TWO OR MORE
//! enemy pieces P1, P2, … such that each Pi is winnable for us once D is removed
//! (best_see_capture on a D-removed, us-to-move probe > 0) but is NOT winnable now
//! (best_see_capture now <= 0) because D guards it. Since D cannot guard both, we win
//! material: capture the lesser Pi (D recaptures, abandoning the greater Pj) or collapse
//! the defence. This REUSES the remove-guard machinery (see
//! `src/facts/motifs.rs::remove_guard_after_move`): the "us-to-move, D gone" probe via
//! `position_for_analysis_side`, and `see()` scored for the side to move.
//!
//! ── PENDING DETECTOR API ─────────────────────────────────────────────────────────
//! These tests target a not-yet-implemented detector. They assume, by analogy with the
//! existing `remove_guard_opportunities` + `RemoveGuardOpportunity`:
//!
//!   pub fn overload_opportunities(pos: &Position) -> FactCollection<OverloadOpportunity>
//!
//!   pub struct OverloadOpportunity {
//!       pub kind: String,                  // "overloading"
//!       pub validator: String,             // "overload_validation"
//!       pub overloaded_defender: PieceRef, // the enemy piece D that cannot guard both
//!       pub targets: Vec<PieceRef>,        // the >=2 enemy pieces D defends, sorted by id
//!       pub material_gain: i32,            // SEE cp we win = the SECOND-best target gain
//!   }
//!
//! If the landed detector names these differently, only the accessor names below change;
//! the FENs and the asserted squares / piece types / material_gain are engine-verified
//! constants (checked against compiled see()/attackers_of()/generate_legal()).
//!
//! material_gain values follow SEE_VALUE (knight 320, bishop 330, rook 500) — the same
//! `best_see_capture` output remove-guard reports — NOT the motif VALUE table (300/300).
//! When the opponent saves the dearer piece, we collect the next-best, so material_gain
//! is the SECOND-highest of the two per-target SEE gains.

use cvs_bitboard_core::facts::motifs::overload_opportunities;
use cvs_bitboard_core::facts::{FactCollection, OverloadOpportunity, PieceType};
use cvs_bitboard_core::Position;

fn ov(fen: &str) -> Vec<OverloadOpportunity> {
    let pos = Position::from_fen(fen).unwrap();
    match overload_opportunities(&pos) {
        FactCollection::Computed { items } => items,
        other => panic!("overload should be computed, got {other:?}"),
    }
}

/// The single overload whose defender sits on `defender_sq` (the battery is built so at
/// most one overload exists per positive case).
fn on_defender<'a>(it: &'a [OverloadOpportunity], defender_sq: &str) -> Option<&'a OverloadOpportunity> {
    it.iter().find(|o| o.overloaded_defender.square == defender_sq)
}

fn target_squares(o: &OverloadOpportunity) -> Vec<&str> {
    o.targets.iter().map(|t| t.square.as_str()).collect()
}

// ── positives (must fire) ─────────────────────────────────────────────────────

#[test]
fn rook_is_sole_defender_of_two_pieces() {
    // (1) Black Rb7 is the ONLY defender of the knight e7 (along rank 7) and the bishop
    // b5 (along the b-file). White Re1 attacks e7, white Rb1 attacks b5. Neither is
    // winnable now — Rb7 recaptures for an even-or-losing exchange. Remove Rb7 and BOTH
    // fall: Rxe7 wins the knight (320), Rxb5 wins the bishop (330). One rook, two duties.
    let items = ov("6k1/1r2n3/8/1b6/8/8/8/1R2R1K1 w - - 0 1");
    let o = on_defender(&items, "b7").expect("Rb7 is overloaded defending e7 and b5");
    assert_eq!(o.kind, "overloading");
    assert_eq!(o.validator, "overload_validation");
    assert_eq!(o.overloaded_defender.piece_type, PieceType::Rook);
    assert_eq!(o.overloaded_defender.square, "b7");
    let mut sqs = target_squares(o);
    sqs.sort();
    assert_eq!(sqs, vec!["b5", "e7"]);
    assert!(o.targets.iter().any(|t| t.square == "e7" && t.piece_type == PieceType::Knight));
    assert!(o.targets.iter().any(|t| t.square == "b5" && t.piece_type == PieceType::Bishop));
    // Opponent saves the dearer bishop (330); we collect the next-best knight (320).
    assert_eq!(o.material_gain, 320);
}

#[test]
fn queen_overloaded_defending_two_squares() {
    // (2) Black Qd6 is the sole defender of the knight d4 (down the d-file) and the
    // bishop a6 (along rank 6) — neither target defends the other, so the queen is the
    // only link. Rd1 attacks d4, Ra1 attacks a6; both are losing captures now (Qxd1/Qxa6
    // recaptures). The queen cannot answer on both wings: removing it wins Rxd4 (320) or
    // Rxa6 (330). Not a check.
    let items = ov("6k1/8/b2q4/8/3n4/8/8/R2R2K1 w - - 0 1");
    let o = on_defender(&items, "d6").expect("Qd6 is overloaded defending d4 and a6");
    assert_eq!(o.overloaded_defender.piece_type, PieceType::Queen);
    assert_eq!(o.overloaded_defender.square, "d6");
    let mut sqs = target_squares(o);
    sqs.sort();
    assert_eq!(sqs, vec!["a6", "d4"]);
    assert!(o.targets.iter().any(|t| t.square == "d4" && t.piece_type == PieceType::Knight));
    assert!(o.targets.iter().any(|t| t.square == "a6" && t.piece_type == PieceType::Bishop));
    assert_eq!(o.material_gain, 320);
}

#[test]
fn classic_capturing_one_target_wins_the_other() {
    // (3) The textbook overload: Black Nc6 is the sole defender of the bishop a7 (Ra1
    // attacks it) and the rook e5 (Re1 attacks it). Capture the lesser piece — Rxa7,
    // Nxa7 — and the knight is dragged off e5, so Rxe5 then wins the rook. (Equivalently
    // Rxe5 first, Nxe5, Rxa7.) The white king sits on h1, off the a7–g1 diagonal, so the
    // bishop gives no check. material_gain is the second-best gain: bishop 330 (we keep
    // at least the bishop's worth; the full swing is 330 + leftover).
    let items = ov("6k1/b7/2n5/4r3/8/8/8/R3R2K w - - 0 1");
    let o = on_defender(&items, "c6").expect("Nc6 is overloaded defending a7 and e5");
    assert_eq!(o.overloaded_defender.piece_type, PieceType::Knight);
    assert_eq!(o.overloaded_defender.square, "c6");
    let mut sqs = target_squares(o);
    sqs.sort();
    assert_eq!(sqs, vec!["a7", "e5"]);
    assert!(o.targets.iter().any(|t| t.square == "a7" && t.piece_type == PieceType::Bishop));
    assert!(o.targets.iter().any(|t| t.square == "e5" && t.piece_type == PieceType::Rook));
    // Targets win 330 (bishop a7) and 500 (rook e5); opponent saves the rook → 330.
    assert_eq!(o.material_gain, 330);
}

#[test]
fn black_exploits_an_overloaded_white_defender() {
    // (9) Colour-mirror of (1): BLACK to move. White Rb2 is the sole defender of the
    // knight e2 (rank 2) and the bishop b4 (b-file); black Re8 attacks e2, black Rb8
    // attacks b4. The detector must not be white-biased.
    let items = ov("1r2r1k1/8/8/8/1B6/8/1R2N3/6K1 b - - 0 1");
    let o = on_defender(&items, "b2").expect("white Rb2 is overloaded defending e2 and b4");
    assert_eq!(o.overloaded_defender.side, cvs_bitboard_core::facts::Side::White);
    assert_eq!(o.overloaded_defender.piece_type, PieceType::Rook);
    assert_eq!(o.overloaded_defender.square, "b2");
    let mut sqs = target_squares(o);
    sqs.sort();
    assert_eq!(sqs, vec!["b4", "e2"]);
    assert_eq!(o.material_gain, 320);
}

#[test]
fn queen_defends_a_rook_and_a_bishop_on_two_lines() {
    // (11) Black Qd6 is the sole defender of the rook a6 (along rank 6) and the bishop f4
    // (down the d6-f4 diagonal). White Ra1 attacks a6, Rf1 attacks f4 — distinct pieces.
    // Regression: the D-removed probe must keep occ[] in sync, else Ra1 sliding onto the
    // vacated d6-defender line makes movegen emit a phantom capture and make() panics.
    let items = ov("6k1/8/r2q4/8/5b2/8/8/R4R1K w - - 0 1");
    let o = on_defender(&items, "d6").expect("Qd6 is overloaded defending a6 and f4");
    assert_eq!(o.overloaded_defender.piece_type, PieceType::Queen);
    let mut sqs = target_squares(o);
    sqs.sort();
    assert_eq!(sqs, vec!["a6", "f4"]);
    // Opponent saves the dearer rook (500); we collect the next-best bishop (330).
    assert_eq!(o.material_gain, 330);
}

#[test]
fn a_pawn_can_be_the_overloaded_defender() {
    // (10) A cheap PAWN as the overloaded defender. Black pawn d6 guards the knight c5
    // and the bishop e5 (it covers both c5 and e5). White Rc1 attacks c5, white Re1
    // attacks e5; each capture loses to dxc5 / dxe5 now. The pawn can recapture on only
    // one square — removing it wins Rxc5 (320) or Rxe5 (330).
    let items = ov("6k1/8/3p4/2n1b3/8/8/8/2R1R1K1 w - - 0 1");
    let o = on_defender(&items, "d6").expect("pawn d6 is overloaded defending c5 and e5");
    assert_eq!(o.overloaded_defender.piece_type, PieceType::Pawn);
    assert_eq!(o.overloaded_defender.square, "d6");
    let mut sqs = target_squares(o);
    sqs.sort();
    assert_eq!(sqs, vec!["c5", "e5"]);
    assert!(o.targets.iter().any(|t| t.square == "c5" && t.piece_type == PieceType::Knight));
    assert!(o.targets.iter().any(|t| t.square == "e5" && t.piece_type == PieceType::Bishop));
    assert_eq!(o.material_gain, 320);
}

// ── adversarial negatives (must NOT fire) ─────────────────────────────────────

#[test]
fn rejects_defender_of_only_one_winnable_target() {
    // (4) Same Rb7 defending the knight e7 (Re1 attacks → winnable once Rb7 leaves) AND
    // the bishop b5 — but b5 has NO white attacker (the b1 rook is gone), so only ONE
    // real winnable target exists. One duty is not an overload.
    let items = ov("6k1/1r2n3/8/1b6/8/8/8/4R1K1 w - - 0 1");
    assert!(
        on_defender(&items, "b7").is_none(),
        "a defender of a single winnable target is not overloaded"
    );
}

#[test]
fn rejects_when_one_target_has_a_second_defender() {
    // (5) Rb7 nominally guards both the knight e7 and the bishop b5, but the bishop d8
    // ALSO defends e7. Removing Rb7 still leaves Bd8 guarding e7, so e7 is not exposed —
    // only b5 is truly Rb7-dependent. Fewer than two genuinely-overloaded targets.
    let items = ov("3b2k1/1r2n3/8/1b6/8/8/8/1R2R1K1 w - - 0 1");
    assert!(
        on_defender(&items, "b7").is_none(),
        "if a target keeps a second defender after D is removed, D is not overloaded by it"
    );
}

#[test]
fn rejects_when_targets_are_winnable_anyway() {
    // (6) Rb7 defends the knight e7 and the bishop b5, but each is ALREADY hanging: a
    // white pawn on d6 attacks e7 (dxe7 wins the knight now, SEE > 0) and a white pawn on
    // a4 attacks b5 (axb5 wins the bishop now). The wins exist with Rb7 still on the
    // board, so removing it is not the cause — no overload.
    let items = ov("6k1/1r2n3/3P4/1b6/P7/8/8/6K1 w - - 0 1");
    assert!(
        on_defender(&items, "b7").is_none(),
        "targets that are already winnable are not won BY the overload"
    );
}

#[test]
fn rejects_when_the_overloaded_defender_is_the_king() {
    // (7) The black king on e4 is the only defender of the knights d4 and f4; white Rd2
    // and Rf2 attack them, and each capture loses to the king's recapture now. If the
    // king were an eligible defender this would be a textbook overload — but the king is
    // excluded (it is a check/mate target, never a piece we win material against), so the
    // detector must stay silent.
    let items = ov("8/8/8/8/3nkn2/8/3R1R2/6K1 w - - 0 1");
    assert!(
        items.iter().all(|o| o.overloaded_defender.piece_type != PieceType::King),
        "the king is never reported as an overloaded defender"
    );
    assert!(items.is_empty(), "king-only overload must produce no fact");
}

#[test]
fn rejects_when_a_candidate_target_is_the_enemy_king() {
    // (8) Rb7 defends the knight e7 (Re1 attacks it → one real winnable target) and also
    // shields the black king on b8 (the king stands on a square Rb7 guards). A naive
    // detector iterating enemy pieces would see the king as a second "defended" target
    // and fire; the king is excluded as a target, leaving exactly one — so no overload.
    let items = ov("1k6/1r2n3/8/8/8/8/8/4R1K1 w - - 0 1");
    assert!(
        on_defender(&items, "b7").is_none(),
        "the enemy king must not be counted as a second overloaded target"
    );
}

// ── invariants ────────────────────────────────────────────────────────────────

#[test]
fn no_overload_in_the_opening_position() {
    assert!(
        ov("rnbqkbnr/pppppppp/8/8/8/8/PPPPPPPP/RNBQKBNR w KQkq - 0 1").is_empty(),
        "the start position has no overloaded defenders"
    );
}

#[test]
fn enumeration_does_not_mutate_the_position() {
    let fen = "6k1/1r2n3/8/1b6/8/8/8/1R2R1K1 w - - 0 1";
    let pos = Position::from_fen(fen).unwrap();
    let _ = overload_opportunities(&pos);
    assert_eq!(
        pos.to_fen(),
        fen,
        "overload enumeration must not mutate the board"
    );
}
