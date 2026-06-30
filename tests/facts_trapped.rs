use cvs_bitboard_core::facts::motifs::trapped_pieces;
use cvs_bitboard_core::facts::{FactCollection, PieceType, TrappedPieceOpportunity};
use cvs_bitboard_core::Position;

fn trapped(fen: &str) -> Vec<TrappedPieceOpportunity> {
    let pos = Position::from_fen(fen).unwrap();
    match trapped_pieces(&pos) {
        FactCollection::Computed { items } => items,
        other => panic!("trapped should be computed, got {other:?}"),
    }
}

fn at<'a>(it: &'a [TrappedPieceOpportunity], sq: &str) -> Option<&'a TrappedPieceOpportunity> {
    it.iter().find(|t| t.piece.square == sq)
}

// ── positives ────────────────────────────────────────────────────────────────

#[test]
fn knight_trapped_in_the_corner() {
    // Black Na1 attacked by Bb2; both escape squares are covered (a2-pawn guards b3,
    // Kd2 guards c2), so the knight is lost.
    let items = trapped("k7/8/8/8/8/8/PB1K4/n7 w - - 0 1");
    let t = at(&items, "a1").expect("the a1 knight should be trapped");
    assert_eq!(t.kind, "trapped_piece");
    assert_eq!(t.validator, "trapped_piece_validation");
    assert_eq!(t.piece.piece_type, PieceType::Knight);
    assert_eq!(t.material_gain, 320);
    assert!(t.attackers.iter().any(|a| a.square == "b2"));
    assert_eq!(t.escape_squares_tried, vec!["b3", "c2"]);
}

#[test]
fn bishop_trapped_on_the_rim() {
    // Black Bh4 attacked by the g3-pawn; its only moves Bxg5 and Bxg3 both lose
    // (f4-pawn re-wins g5, h2-pawn re-wins g3).
    let items = trapped("6k1/8/8/6P1/5P1b/6P1/7P/6K1 w - - 0 1");
    let t = at(&items, "h4").expect("the h4 bishop should be trapped");
    assert_eq!(t.piece.piece_type, PieceType::Bishop);
    assert_eq!(t.material_gain, 330);
    assert!(t.attackers.iter().any(|a| a.square == "g3"));
}

// ── negatives (must NOT report a trap) ───────────────────────────────────────

#[test]
fn not_trapped_when_the_piece_has_a_safe_retreat() {
    // Knight e4 is attacked by Bh1 but has eight hops, several safe.
    assert!(at(&trapped("7k/8/8/8/4n3/8/8/6KB w - - 0 1"), "e4").is_none());
}

#[test]
fn not_trapped_when_the_piece_can_slide_away() {
    // Rook d5 is attacked by Rd2 but slides anywhere along rank 5.
    assert!(at(&trapped("7k/8/8/3r4/8/8/3R4/6K1 w - - 0 1"), "d5").is_none());
}

#[test]
fn not_trapped_when_it_can_capture_its_attacker_to_safety() {
    // Bishop e3 is attacked by the d2-pawn but Bxd2 is safe (and it has retreats).
    assert!(at(&trapped("7k/8/8/8/8/4b3/3P4/6K1 w - - 0 1"), "e3").is_none());
}

#[test]
fn not_trapped_when_adequately_defended() {
    // Knight e6 is attacked by Re2 but defended by the f7-pawn — not winnable in place.
    assert!(at(&trapped("7k/5p2/4n3/8/8/8/4R3/6K1 w - - 0 1"), "e6").is_none());
}

#[test]
fn not_trapped_when_not_even_attacked() {
    assert!(trapped("7k/8/8/8/3n4/8/8/6K1 w - - 0 1").is_empty());
}

#[test]
fn never_reports_a_king_or_pawn() {
    // The detector iterates only N/B/R/Q enemy pieces — no king/pawn facts ever.
    for fen in [
        "6k1/5ppp/8/8/8/8/5PPP/4R1K1 w - - 0 1",
        "6k1/8/8/6P1/5P1b/6P1/7P/6K1 w - - 0 1",
    ] {
        for t in trapped(fen) {
            assert_ne!(t.piece.piece_type, PieceType::King);
            assert_ne!(t.piece.piece_type, PieceType::Pawn);
        }
    }
}

#[test]
fn no_trapped_pieces_in_the_opening_position() {
    assert!(trapped("rnbqkbnr/pppppppp/8/8/8/8/PPPPPPPP/RNBQKBNR w KQkq - 0 1").is_empty());
}

#[test]
fn enumeration_does_not_mutate_the_position() {
    let fen = "k7/8/8/8/8/8/PB1K4/n7 w - - 0 1";
    let pos = Position::from_fen(fen).unwrap();
    let _ = trapped_pieces(&pos);
    assert_eq!(pos.to_fen(), fen, "trapped enumeration must not mutate the board");
}
