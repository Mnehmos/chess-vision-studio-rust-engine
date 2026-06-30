use cvs_bitboard_core::facts::mate_patterns::mate_pattern_opportunities;
use cvs_bitboard_core::facts::{FactCollection, MatePatternFact, PieceType};
use cvs_bitboard_core::Position;

fn mates(fen: &str) -> Vec<MatePatternFact> {
    let pos = Position::from_fen(fen).unwrap();
    match mate_pattern_opportunities(&pos) {
        FactCollection::Computed { items } => items,
        other => panic!("mate patterns should be computed, got {other:?}"),
    }
}

fn find<'a>(it: &'a [MatePatternFact], uci: &str) -> Option<&'a MatePatternFact> {
    it.iter().find(|m| m.move_uci == uci)
}

// ── positives ────────────────────────────────────────────────────────────────

#[test]
fn classifies_a_back_rank_mate() {
    // Ra1-a8#: the king on g8 is hemmed by its own f7/g7/h7 pawns; the rook mates
    // along the 8th rank.
    let items = mates("6k1/5ppp/8/8/8/8/8/R5K1 w - - 0 1");
    let m = find(&items, "a1a8").expect("Ra8 should be a classified back-rank mate");
    assert_eq!(m.kind, "back_rank_mate");
    assert_eq!(m.validator, "mate_pattern_validation");
    assert_eq!(m.mating_piece.piece_type, PieceType::Rook);
    assert_eq!(m.mating_piece.square, "a8");
    assert_eq!(m.mated_king.square, "g8");
    assert!(m.gives_check);
    assert!(m.key_squares.contains(&"g7".to_string()));
}

#[test]
fn classifies_a_smothered_mate() {
    // Ng5-f7#: the king on h8 is fully smothered by its own rook g8 and pawns g7/h7.
    let items = mates("6rk/6pp/8/6N1/8/8/8/6K1 w - - 0 1");
    let m = find(&items, "g5f7").expect("Nf7 should be a classified smothered mate");
    assert_eq!(m.kind, "smothered_mate");
    assert_eq!(m.mating_piece.piece_type, PieceType::Knight);
    assert_eq!(m.mating_piece.square, "f7");
    assert_eq!(m.mated_king.square, "h8");
    assert!(m.gives_check);
}

// ── negatives / precision ────────────────────────────────────────────────────

#[test]
fn does_not_misclassify_a_lawnmower_ladder_mate() {
    // Two rooks (Ra7 + Rb1-b8#) COVER the king's escapes rather than the king being
    // hemmed by its OWN pawns — that is a lawnmower/ladder mate (a later batch), not a
    // back-rank mate. Precision over recall: it must NOT be labelled back_rank_mate.
    let items = mates("7k/R7/8/8/8/8/8/1R4K1 w - - 0 1");
    assert!(items.iter().all(|m| m.kind != "back_rank_mate"));
    assert!(items.iter().all(|m| m.kind != "smothered_mate"));
}

#[test]
fn returns_none_for_an_unbatched_mate_pattern() {
    // King-and-queen box mate (Qe7-g7#): not a back-rank (queen off the king's rank)
    // and not smothered (queen, not knight) — no first-batch pattern matches.
    assert!(mates("7k/4Q3/6K1/8/8/8/8/8 w - - 0 1").is_empty());
}

#[test]
fn no_mate_patterns_when_there_is_no_mate() {
    // Ra8+ is only a check here — the king escapes to the empty h7 — so no mate fact.
    assert!(mates("6k1/5pp1/8/8/8/8/8/R5K1 w - - 0 1").is_empty());
}

#[test]
fn no_mate_patterns_in_the_opening_position() {
    assert!(mates("rnbqkbnr/pppppppp/8/8/8/8/PPPPPPPP/RNBQKBNR w KQkq - 0 1").is_empty());
}

#[test]
fn enumeration_does_not_mutate_the_position() {
    let fen = "6k1/5ppp/8/8/8/8/8/R5K1 w - - 0 1";
    let pos = Position::from_fen(fen).unwrap();
    let _ = mate_pattern_opportunities(&pos);
    assert_eq!(pos.to_fen(), fen, "mate-pattern enumeration must not mutate the board");
}
