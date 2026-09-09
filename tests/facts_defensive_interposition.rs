//! Adversarial FEN battery for the DEFENSIVE INTERPOSITION detector (ChessTempo
//! "Defensive Interposition": block the line between an enemy attacker and your own piece).
//!
//! A MOVE detector, the defensive mirror of interference: a move lands on a square S strictly
//! between an enemy SLIDER A and OUR piece G, severing an attack that was legally winning G.
//! Trigger/proof mirror discovered_defense (G losing before, no longer legally winnable after);
//! the load-bearing FP guard is the full-board legal capture/promotion quiescence, which debits
//! an underdefended interposer, the AxS-recapture-reopens-line pitfall, and posts abandoned by
//! the mover. material_gain is the enemy's pre-move winning SEE on G (SEE_VALUE scale:
//! pawn 100, knight 320, bishop 330, rook 500, queen 900).

use cvs_bitboard_core::facts::motifs::defensive_interposition_opportunities;
use cvs_bitboard_core::facts::{DefensiveInterpositionOpportunity, FactCollection, PieceType};
use cvs_bitboard_core::Position;

fn di(fen: &str) -> Vec<DefensiveInterpositionOpportunity> {
    let pos = Position::from_fen(fen).unwrap();
    match defensive_interposition_opportunities(&pos) {
        FactCollection::Computed { items } => items,
        other => panic!("defensive interposition should be computed, got {other:?}"),
    }
}

fn by_move<'a>(
    it: &'a [DefensiveInterpositionOpportunity],
    uci: &str,
) -> Option<&'a DefensiveInterpositionOpportunity> {
    it.iter().find(|o| o.move_uci == uci)
}

// ── positives (must fire) ─────────────────────────────────────────────────────

#[test]
fn knight_blocks_a_rook_file_to_save_a_hanging_bishop() {
    // Black Rd8 attacks the undefended Bd2 down the open d-file (Rxd2 wins 330 clean).
    // Ne2-d4 interposes on d4, held by Pe3 (Rxd4 exd4 = -180): the bishop is saved.
    let items = di("3r2k1/8/8/8/8/4P3/3BN3/6K1 w - - 0 1");
    assert_eq!(items.len(), 1, "exactly one interposition: {items:?}");
    let o = by_move(&items, "e2d4").expect("Ne2-d4 blocks Rd8's attack on Bd2");
    assert_eq!(o.kind, "defensive_interposition");
    assert_eq!(o.validator, "defensive_interposition_validation");
    assert_eq!(o.interposer.piece_type, PieceType::Knight);
    assert_eq!(o.interposer.square, "d4");
    assert_eq!(o.cut_attacker.piece_type, PieceType::Rook);
    assert_eq!(o.cut_attacker.square, "d8");
    assert_eq!(o.protected.piece_type, PieceType::Bishop);
    assert_eq!(o.protected.square, "d2");
    assert_eq!(o.ray, vec!["d7", "d6", "d5", "d4", "d3"]);
    assert!(!o.gives_check);
    assert_eq!(o.material_gain, 330);
}

#[test]
fn pawn_push_blocks_a_bishop_diagonal_to_save_a_hanging_rook() {
    // Black Ba8 attacks the undefended Rf3 along the open long diagonal (Bxf3 wins 500).
    // e3-e4 interposes on e4, held by Pd3 (Bxe4 dxe4 = -230): the rook is saved.
    let items = di("b6k/8/8/8/8/3PPR2/8/6K1 w - - 0 1");
    assert_eq!(items.len(), 1, "exactly one interposition: {items:?}");
    let o = by_move(&items, "e3e4").expect("e3-e4 blocks Ba8's attack on Rf3");
    assert_eq!(o.interposer.piece_type, PieceType::Pawn);
    assert_eq!(o.interposer.square, "e4");
    assert_eq!(o.cut_attacker.piece_type, PieceType::Bishop);
    assert_eq!(o.cut_attacker.square, "a8");
    assert_eq!(o.protected.piece_type, PieceType::Rook);
    assert_eq!(o.protected.square, "f3");
    assert_eq!(o.ray, vec!["b7", "c6", "d5", "e4"]);
    assert!(!o.gives_check);
    assert_eq!(o.material_gain, 500);
}

#[test]
fn check_giving_interposition_is_supported() {
    // Black Rh4 attacks the undefended Bb4 along the open fourth rank. Nf2-e4+ interposes AND
    // checks Kg5; the only capture-evasion Rxe4 loses to dxe4 (-180). No us-to-move probe is
    // built, so — unlike remove_guard/xray/deflection — a check-giving save fully fires.
    let items = di("8/8/8/6k1/1B5r/3P4/5N2/6K1 w - - 0 1");
    assert_eq!(items.len(), 1, "exactly one interposition: {items:?}");
    let o = by_move(&items, "f2e4").expect("Nf2-e4+ blocks Rh4's attack on Bb4 with check");
    assert_eq!(o.interposer.piece_type, PieceType::Knight);
    assert_eq!(o.interposer.square, "e4");
    assert_eq!(o.cut_attacker.square, "h4");
    assert_eq!(o.protected.piece_type, PieceType::Bishop);
    assert_eq!(o.protected.square, "b4");
    assert_eq!(o.ray, vec!["g4", "f4", "e4", "d4", "c4"]);
    assert!(o.gives_check);
    assert_eq!(o.material_gain, 330);
    // The hanging alternatives on the same rank must NOT fire: d3-d4 and Nf2-g4 both leave the
    // interposer takeable for profit (Rxd4 +100; Kxg4/Rxg4 +320).
    assert!(by_move(&items, "d3d4").is_none());
    assert!(by_move(&items, "f2g4").is_none());
}

#[test]
fn only_the_affordable_interposer_fires_and_flight_is_not_a_block() {
    // Black Ba8 attacks the undefended Qf3 (900). e3-e4 blocks on e4 (held by Pd3 AND Rb4;
    // Bxe4 dxe4 = -230) and fires. In the SAME position the rook interpositions must be
    // ABSENT: b4e4 is refuted by a8xe4 (+170 — the bishop eats the too-expensive interposer
    // and from e4 renews the attack on f3: the AxS reopen), b4b7 by a8xb7 (+500). The queen
    // sliding down its own attack ray (f3e4/f3d5) is flight, not a block (g_sq == mv.from).
    let items = di("b6k/8/8/8/1R6/3PPQ2/8/6K1 w - - 0 1");
    assert_eq!(items.len(), 1, "exactly one interposition: {items:?}");
    let o = by_move(&items, "e3e4").expect("e3-e4 blocks Ba8's attack on Qf3");
    assert_eq!(o.protected.piece_type, PieceType::Queen);
    assert_eq!(o.protected.square, "f3");
    assert_eq!(o.material_gain, 900);
    assert!(
        by_move(&items, "b4e4").is_none(),
        "rook interposer hangs to a8xe4 (+170)"
    );
    assert!(
        by_move(&items, "b4b7").is_none(),
        "rook interposer hangs to a8xb7 (+500)"
    );
    assert!(
        by_move(&items, "f3e4").is_none(),
        "queen flight along the ray is not a block"
    );
    assert!(
        by_move(&items, "f3d5").is_none(),
        "queen flight along the ray is not a block"
    );
}

// ── adversarial negatives (must produce ZERO ops) ─────────────────────────────

#[test]
fn rejects_when_the_interposer_simply_hangs() {
    // The knight-block position WITHOUT the e3 pawn: Ne2-d4 is refuted by Rd8xd4 (+320 — the
    // rook eats the undefended interposer, lands ON S, and re-attacks d2).
    assert!(di("3r2k1/8/8/8/8/8/3BN3/6K1 w - - 0 1").is_empty());
}

#[test]
fn rejects_when_a_second_attacker_still_wins_the_piece() {
    // The knight-block position PLUS a black knight on b3: Nb3 also wins Bd2, so blocking the
    // rook's line saves nothing (refuting reply b3xd2). Causality — deleting Rd8 must kill the
    // legal win on d2 — rejects every candidate.
    assert!(di("3r2k1/8/8/8/8/1n2P3/3BN3/6K1 w - - 0 1").is_empty());
}

#[test]
fn rejects_when_the_piece_was_never_losing() {
    // The knight-block position PLUS a white knight on b1 defending d2: Rxd2 = 330 - 500 < 0,
    // so there is no threat to sever — nothing to save, no trigger.
    assert!(di("3r2k1/8/8/8/8/4P3/3BN3/1N4K1 w - - 0 1").is_empty());
}

#[test]
fn rejects_when_the_block_abandons_another_post() {
    // LOAD-BEARING quiescence guard (off-square collateral): Ne2-d4 blocks Rd8's attack on Rd2
    // and every single-square gate passes (d4 is held by Pc3; d2 is no longer reachable), but
    // Ne2 was Ng3's only defender — the refuting reply is h4xg3 (+320 free).
    assert!(di("3r3k/8/8/8/7b/2P3N1/3RN3/6K1 w - - 0 1").is_empty());
}

#[test]
fn rejects_a_self_defeating_block_by_the_sole_defender() {
    // Black Bh1 attacks Rd5 through e4 (Bxd5 nets +170 against the Nc3 recapture), and Rd8
    // piles on the d-file. Nc3-e4 blocks the bishop's diagonal on a held square (Bxe4 dxe4 =
    // -10) — but the knight WAS d5's sole defender, so the block abandons it: the refuting
    // reply is d8xd5 (+500 free). PROOF-1 (G still legally winnable after the move) rejects.
    // The rook pair alone is also rejected by causality (without Rd8 the bishop still wins).
    assert!(di("3r3k/8/8/3R4/8/2NP4/8/6Kb w - - 0 1").is_empty());
}

#[test]
fn rejects_a_double_push_interposer_capturable_en_passant() {
    // Black Bb7 attacks Rf3 through e4 (Bxf3 exf3 = +170). e2-e4 blocks on a held square
    // (Bxe4 dxe4 = -230) and every capture-onto-a-square gate passes — but the block is a
    // DOUBLE PUSH and black replies f4xe3/d4xe3 EN PASSANT: the recapture does NOT land on S,
    // wins the pawn, and re-opens the diagonal with the e3 pawn protected. The EP guard
    // rejects the move outright.
    assert!(di("k7/1b6/8/8/3p1p2/3P1R2/4P3/6K1 w - - 0 1").is_empty());
}

#[test]
fn no_defensive_interposition_in_the_opening_position() {
    assert!(di("rnbqkbnr/pppppppp/8/8/8/8/PPPPPPPP/RNBQKBNR w KQkq - 0 1").is_empty());
}

// ── invariant ─────────────────────────────────────────────────────────────────

#[test]
fn enumeration_does_not_mutate_the_position() {
    let fen = "3r2k1/8/8/8/8/4P3/3BN3/6K1 w - - 0 1";
    let pos = Position::from_fen(fen).unwrap();
    let _ = defensive_interposition_opportunities(&pos);
    assert_eq!(
        pos.to_fen(),
        fen,
        "defensive interposition enumeration must not mutate the board"
    );
}
