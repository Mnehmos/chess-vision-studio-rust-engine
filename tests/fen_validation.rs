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

// --- Audit #62: castling rights without home pieces, pawns on back ranks ---

#[test]
fn rejects_king_right_without_home_pieces() {
    // 'K' claimed but the white rook is on f1 (h1 empty): gen_castling would
    // fabricate a rook on f1 during make().
    let err = Position::from_fen("4k3/8/8/8/8/8/5R2/4K3 w K - 0 1")
        .err()
        .expect("castling right without rook must be rejected");
    assert!(err.contains("'K'"), "got: {err}");
}

#[test]
fn rejects_queen_right_without_rook() {
    let err = Position::from_fen("4k3/8/8/8/8/8/8/4K2R w Q - 0 1")
        .err()
        .expect("'Q' without a1 rook must be rejected");
    assert!(err.contains("'Q'"), "got: {err}");
}

#[test]
fn rejects_black_rights_without_home_pieces() {
    let err = Position::from_fen("4k3/8/8/8/8/8/8/4K2R w kq - 0 1")
        .err()
        .expect("black rights without black rooks must be rejected");
    assert!(err.contains("'k'") || err.contains("'q'"), "got: {err}");
}

#[test]
fn rejects_right_after_rook_captured_over_fen() {
    // Rights retained in a FEN whose rook was already captured elsewhere.
    let err = Position::from_fen("4k3/8/8/8/8/8/8/R3K3 w KQ - 0 1")
        .err()
        .expect("'K' without h1 rook must be rejected");
    assert!(err.contains("'K'"), "got: {err}");
}

#[test]
fn accepts_valid_castling_rights() {
    // The full legal castling setup must still parse.
    assert!(Position::from_fen("r3k2r/8/8/8/8/8/8/R3K2R w KQkq - 0 1").is_ok());
}

#[test]
fn rejects_pawn_on_back_rank() {
    // White pawn on rank 1: movegen's from - 8 / from + 8 arithmetic wraps.
    let err = Position::from_fen("4k3/8/8/8/8/8/8/P3K3 w - - 0 1")
        .err()
        .expect("rank-1 pawn must be rejected");
    assert!(err.contains("rank 1/8"), "got: {err}");
    // Black pawn on rank 8.
    let err = Position::from_fen("p3k3/8/8/8/8/8/8/4K3 b - - 0 1")
        .err()
        .expect("rank-8 pawn must be rejected");
    assert!(err.contains("rank 1/8"), "got: {err}");
}
