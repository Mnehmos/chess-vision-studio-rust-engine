use cvs_bitboard_core::facts::motifs::remove_guard_opportunities;
use cvs_bitboard_core::facts::{FactCollection, PieceType, RemoveGuardOpportunity};
use cvs_bitboard_core::Position;

fn rg(fen: &str) -> Vec<RemoveGuardOpportunity> {
    let pos = Position::from_fen(fen).unwrap();
    match remove_guard_opportunities(&pos) {
        FactCollection::Computed { items } => items,
        other => panic!("remove-guard should be computed, got {other:?}"),
    }
}

fn find<'a>(it: &'a [RemoveGuardOpportunity], uci: &str) -> Option<&'a RemoveGuardOpportunity> {
    it.iter().find(|d| d.move_uci == uci)
}

// ── positive ─────────────────────────────────────────────────────────────────

#[test]
fn captures_the_sole_defender_then_the_target_is_winnable() {
    // Rook d8 is attacked by Rd1 and defended ONLY by the knight c6 (SEE 0 — not yet
    // winnable). Bg2xc6 removes the guard; now Rxd8 wins the undefended rook.
    let items = rg("3r2k1/8/2n5/8/8/8/6B1/3R2K1 w - - 0 1");
    let d = find(&items, "g2c6").expect("Bxc6 should remove the guard of the d8 rook");
    assert_eq!(d.kind, "capture_the_defender");
    assert_eq!(d.validator, "remove_guard_validation");
    assert_eq!(d.captured_defender.piece_type, PieceType::Knight);
    assert_eq!(d.captured_defender.square, "c6");
    assert_eq!(d.target.piece_type, PieceType::Rook);
    assert_eq!(d.target.square, "d8");
    assert!(!d.gives_check);
    assert_eq!(d.material_gain, 500);
}

// ── negatives (must NOT fire) ────────────────────────────────────────────────

#[test]
fn rejects_when_target_still_has_a_second_defender() {
    // d8 is guarded by BOTH the knight c6 and the bishop c7; capturing the knight
    // still leaves the bishop, so Rxd8 is only an even trade — not winnable.
    let items = rg("3r2k1/2b5/2n5/8/8/8/6B1/3R2K1 w - - 0 1");
    assert!(find(&items, "g2c6").is_none());
}

#[test]
fn rejects_when_the_capturer_simply_hangs() {
    // Bg2xc6 wins the defender, but the black b7-pawn plays bxc6 winning the bishop.
    let items = rg("3r2k1/1p6/2n5/8/8/8/6B1/3R2K1 w - - 0 1");
    assert!(find(&items, "g2c6").is_none());
}

#[test]
fn rejects_when_the_target_was_already_winnable() {
    // Doubled rooks already win the d8 rook (SEE > 0) regardless of the knight, so
    // capturing the "guard" is not the cause — no remove-guard fact.
    let items = rg("3r2k1/8/2n5/8/8/3R4/6B1/3R2K1 w - - 0 1");
    assert!(find(&items, "g2c6").is_none());
}

#[test]
fn rejects_when_the_captured_piece_was_not_a_defender() {
    // Knight e4 does not guard the d8 rook; Bxe4 removes no relevant guard.
    let items = rg("3r2k1/8/8/8/4n3/8/6B1/3R2K1 w - - 0 1");
    assert!(find(&items, "g2e4").is_none());
}

#[test]
fn no_remove_guard_in_the_opening_position() {
    assert!(rg("rnbqkbnr/pppppppp/8/8/8/8/PPPPPPPP/RNBQKBNR w KQkq - 0 1").is_empty());
}

#[test]
fn enumeration_does_not_mutate_the_position() {
    let fen = "3r2k1/8/2n5/8/8/8/6B1/3R2K1 w - - 0 1";
    let pos = Position::from_fen(fen).unwrap();
    let _ = remove_guard_opportunities(&pos);
    assert_eq!(pos.to_fen(), fen, "remove-guard enumeration must not mutate the board");
}
