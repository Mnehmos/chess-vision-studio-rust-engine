//! from_fen must reject structurally illegal positions — search assumes both
//! kings exist and the side to move cannot already capture the enemy king.
//! Regression: an illegal analysis FEN (idle king en prise) reached quiesce and
//! panicked in pawn_attacks with square 64 after the king was captured.
use cvs_bitboard_core::position::Position;

#[test]
fn rejects_idle_side_in_check() {
    // White to move, but Bd4 already attacks the black king on g7.
    let err = Position::from_fen("8/5pk1/6p1/8/3B4/6P1/5PK1/3b4 w - - 0 40")
        .err()
        .expect("illegal FEN must be rejected");
    assert!(err.contains("in check"), "got: {err}");
}

#[test]
fn rejects_missing_king() {
    let err = Position::from_fen("8/5pk1/6p1/8/3B4/6P1/5P2/3b4 w - - 0 40")
        .err()
        .expect("kingless FEN must be rejected");
    assert!(err.contains("exactly one king"), "got: {err}");
}

#[test]
fn accepts_checks_against_side_to_move() {
    // Same shape but Black to move: a normal check, perfectly legal.
    let pos = Position::from_fen("8/5pk1/6p1/8/3B4/6P1/5PK1/3b4 b - - 0 40");
    assert!(pos.is_ok());
}
