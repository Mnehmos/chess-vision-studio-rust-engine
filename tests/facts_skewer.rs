use cvs_bitboard_core::facts::motifs::skewer_opportunities;
use cvs_bitboard_core::facts::{FactCollection, PieceType, SkewerOpportunity};
use cvs_bitboard_core::Position;

fn skewers(fen: &str) -> Vec<SkewerOpportunity> {
    let pos = Position::from_fen(fen).unwrap();
    match skewer_opportunities(&pos) {
        FactCollection::Computed { items } => items,
        other => panic!("skewers should be computed, got {other:?}"),
    }
}

#[test]
fn validates_a_rook_skewer_of_king_winning_the_rook_behind() {
    // White to move: Ra1-e1+ skewers the black king on e4; when it steps off the
    // e-file the (undefended) rook on e8 behind it falls.
    let items = skewers("4r3/8/8/8/4k3/8/8/R6K w - - 0 1");
    let sk = items
        .iter()
        .find(|s| s.move_uci == "a1e1")
        .expect("Re1+ king skewer should be validated");
    assert_eq!(sk.kind, "skewer");
    assert_eq!(sk.validator, "skewer_validation");
    assert_eq!(sk.front.piece_type, PieceType::King);
    assert_eq!(sk.front.square, "e4");
    assert_eq!(sk.back.piece_type, PieceType::Rook);
    assert_eq!(sk.back.square, "e8");
    assert!(sk.gives_check);
    assert_eq!(sk.material_gain, 500);
}

#[test]
fn validates_a_relative_skewer_of_queen_then_rook() {
    // White to move: Ba1-b2 attacks the black queen on d4 (worth more than the
    // bishop, and Qxb2 loses the queen to Kxb2), so the queen must move and abandon
    // the rook on f6 behind it. Not a check.
    let items = skewers("7k/8/5r2/8/3q4/8/8/B1K5 w - - 0 1");
    let sk = items
        .iter()
        .find(|s| s.move_uci == "a1b2")
        .expect("relative skewer of queen→rook should be validated");
    assert_eq!(sk.front.piece_type, PieceType::Queen);
    assert_eq!(sk.front.square, "d4");
    assert_eq!(sk.back.piece_type, PieceType::Rook);
    assert_eq!(sk.back.square, "f6");
    assert!(!sk.gives_check);
    assert_eq!(sk.material_gain, 500);
}

#[test]
fn does_not_report_a_pin_as_a_skewer() {
    // Ba1-b2 attacks the black rook on e5 with the black queen on h8 behind it: the
    // front piece is LESS valuable than the piece behind, so it is a pin, not a skewer.
    let items = skewers("k6q/8/8/4r3/8/8/8/B6K w - - 0 1");
    assert!(
        items.iter().all(|s| s.move_uci != "a1b2"),
        "a rook pinned to a queen must not be reported as a skewer"
    );
}

#[test]
fn rejects_a_skewer_whose_skewerer_is_simply_captured() {
    // Ra1-e1+ would skewer king+rook, but the black bishop on a5 plays Bxe1
    // (resolving the check and winning the rook): the skewerer is simply lost.
    let items = skewers("4r3/8/8/b7/4k3/8/8/R6K w - - 0 1");
    assert!(
        items.iter().all(|s| s.move_uci != "a1e1"),
        "a skewerer that is immediately captured for gain is not a skewer"
    );
}

#[test]
fn no_skewers_in_the_opening_position() {
    assert!(
        skewers("rnbqkbnr/pppppppp/8/8/8/8/PPPPPPPP/RNBQKBNR w KQkq - 0 1").is_empty(),
        "the start position has no skewers"
    );
}

#[test]
fn enumeration_does_not_mutate_the_position() {
    let fen = "4r3/8/8/8/4k3/8/8/R6K w - - 0 1";
    let pos = Position::from_fen(fen).unwrap();
    let _ = skewer_opportunities(&pos);
    assert_eq!(
        pos.to_fen(),
        fen,
        "skewer enumeration must not mutate the board"
    );
}
