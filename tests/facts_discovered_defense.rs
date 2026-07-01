use cvs_bitboard_core::facts::motifs::discovered_defense_opportunities;
use cvs_bitboard_core::facts::{DiscoveredDefenseOpportunity, FactCollection, PieceType};
use cvs_bitboard_core::Position;

fn defenses(fen: &str) -> Vec<DiscoveredDefenseOpportunity> {
    let pos = Position::from_fen(fen).unwrap();
    match discovered_defense_opportunities(&pos) {
        FactCollection::Computed { items } => items,
        other => panic!("discovered defenses should be computed, got {other:?}"),
    }
}

fn find<'a>(
    items: &'a [DiscoveredDefenseOpportunity],
    uci: &str,
) -> Option<&'a DiscoveredDefenseOpportunity> {
    items.iter().find(|d| d.move_uci == uci)
}

// ── positives ────────────────────────────────────────────────────────────────

#[test]
fn rook_unveiled_to_defend_a_hanging_bishop() {
    // Nd2-f3 vacates the d-file; rook d1 now defends bishop d3, which the enemy rook d8
    // was winning for free before the move.
    let items = defenses("3r2k1/8/8/8/8/3B4/3N4/3R2K1 w - - 0 1");
    let d = find(&items, "d2f3").expect("Nf3 should discover the rook's defense of d3");
    assert_eq!(d.kind, "discovered_defense");
    assert_eq!(d.validator, "discovered_defense_validation");
    assert_eq!(d.slider.piece_type, PieceType::Rook);
    assert_eq!(d.slider.square, "d1");
    assert_eq!(d.defended_piece.piece_type, PieceType::Bishop);
    assert_eq!(d.defended_piece.square, "d3");
    assert_eq!(d.mover.piece_type, PieceType::Knight);
    assert!(!d.gives_check);
    assert_eq!(d.material_gain, 330);
    assert_eq!(d.ray, vec!["d2".to_string()]);
}

#[test]
fn bishop_unveiled_diagonally_to_defend_a_hanging_knight() {
    // Nc3-b5 vacates the a1–h8 diagonal; bishop b2 now defends knight d4, which the enemy
    // rook d8 was winning down the d-file before the move.
    let items = defenses("3r2k1/8/8/8/3N4/2N5/1B6/6K1 w - - 0 1");
    let d = find(&items, "c3b5").expect("Nb5 should discover the bishop's defense of d4");
    assert_eq!(d.kind, "discovered_defense");
    assert_eq!(d.slider.piece_type, PieceType::Bishop);
    assert_eq!(d.slider.square, "b2");
    assert_eq!(d.defended_piece.piece_type, PieceType::Knight);
    assert_eq!(d.defended_piece.square, "d4");
    assert!(!d.gives_check);
    assert_eq!(d.material_gain, 320);
}

#[test]
fn queen_unveiled_along_a_file_to_defend_a_hanging_rook_with_check() {
    // Na4-b6 vacates the a-file so queen a1 defends the hanging rook a6, and the knight
    // hop simultaneously checks the enemy king c8.
    let items = defenses("2k5/8/R1r5/8/N7/8/8/Q3K3 w - - 0 1");
    let d = find(&items, "a4b6").expect("Nb6+ should discover the queen's defense of a6");
    assert_eq!(d.kind, "discovered_defense");
    assert_eq!(d.slider.piece_type, PieceType::Queen);
    assert_eq!(d.slider.square, "a1");
    assert_eq!(d.defended_piece.piece_type, PieceType::Rook);
    assert_eq!(d.defended_piece.square, "a6");
    assert!(d.gives_check);
    assert_eq!(d.material_gain, 500);
}

#[test]
fn a_hanging_pawn_is_a_legitimate_rescue_target() {
    // Pawns are permitted as the defended piece; Nd2-f3 unveils rook d1 onto the hanging
    // pawn d3.
    let items = defenses("3r2k1/8/8/8/8/3P4/3N4/3R2K1 w - - 0 1");
    let d = find(&items, "d2f3").expect("Nf3 should rescue the hanging pawn d3");
    assert_eq!(d.defended_piece.piece_type, PieceType::Pawn);
    assert_eq!(d.defended_piece.square, "d3");
    assert_eq!(d.material_gain, 100);
}

// ── negatives (must NOT fire) ────────────────────────────────────────────────

#[test]
fn rejects_when_the_piece_was_never_hanging() {
    // Nothing on the d-file above the knight is under attack, so the unveiled rook defends
    // a piece that never needed rescuing → no fact (causality guard).
    let items = defenses("3r2k1/8/8/8/8/8/3N4/3R2K1 w - - 0 1");
    assert!(items.is_empty());
}

#[test]
fn rejects_an_illusory_rescue_that_hangs_the_mover() {
    // Nd2-c4 would unveil rook d1 onto bishop d3, but the knight lands on c4 where the
    // enemy pawn b5 wins it for free — the "rescue" trades one hang for another, so guard
    // (a) rejects the c4 move (the non-hanging unveils still fire).
    let items = defenses("3r2k1/8/8/1p6/8/3B4/3N4/3R2K1 w - - 0 1");
    assert!(find(&items, "d2c4").is_none());
    assert!(
        find(&items, "d2f3").is_some(),
        "a non-hanging unveil should still fire in the same position"
    );
}

#[test]
fn rejects_a_mover_sliding_along_the_unveiled_ray() {
    // Rd4 sits on bishop b2's a1–h8 diagonal in front of hanging Nf6. Rd4-e5 stays ON the
    // diagonal (re-blocks b2), so nothing is unveiled; only the OFF-diagonal Rd4-e4 rescues.
    let items = defenses("5r1k/8/5N2/8/3R4/8/1B6/4K3 w - - 0 1");
    assert!(find(&items, "d4e5").is_none());
    assert!(find(&items, "d4c3").is_none());
    assert!(
        find(&items, "d4e4").is_some(),
        "the off-diagonal unveil should rescue the knight"
    );
}

#[test]
fn rejects_when_the_defense_is_insufficient_and_the_piece_still_hangs() {
    // Bishop d3 is attacked by TWO enemy rooks (d8 and d7); unveiling rook d1 adds only one
    // defender, so best_see_capture stays positive after the move → still hanging, no fact.
    let items = defenses("3r2k1/3r4/8/8/8/3B4/3N4/3R2K1 w - - 0 1");
    assert!(items.is_empty());
}

#[test]
fn rejects_a_pinned_unveiled_slider() {
    // Nd2-f3 unveils rook d1 "onto" the hanging bishop d3, but Rd1 is ABSOLUTELY PINNED along
    // rank 1 to Kf1 by Rb1 — it can never legally recapture on d3. best_see_capture ignores the
    // pin (fake rescue); the legality-aware gate rejects it. Regression for the fuzz-found FP.
    let items = defenses("3r2k1/8/8/8/8/3BN3/3N4/1r1R1K2 w - - 0 1");
    assert!(
        items.iter().all(|d| d.move_uci != "d2f3"),
        "a pinned unveiled slider cannot rescue, got {items:?}"
    );
}

#[test]
fn rejects_a_too_valuable_unveiled_defender_that_still_loses_the_exchange() {
    // Kd7-c7 unveils the queen d8 to "defend" pawn d5, but d5 has three white attackers
    // (c4, e4 pawns + Nf4) and the queen cannot be the last recapturer (Nxd5 wins her), so d5
    // is legally lost anyway. The legality-aware SEE (standard stop rule) rejects it while the
    // pin-blind best_see_capture accepted it — the second fuzz-found FP class.
    let items = defenses("rn1q2nr/3k4/2p2p1p/pP1p4/Pb1pPNP1/2Q4P/1P2B1P1/R3K1NR b - - 3 19");
    assert!(
        items.iter().all(|d| d.move_uci != "d7c7" && d.move_uci != "d7c8"),
        "a too-valuable unveiled defender does not rescue, got {items:?}"
    );
}

#[test]
fn rejects_when_the_rear_piece_is_a_non_slider() {
    // The piece behind the knight on d1 is another knight, not a slider — nothing can be
    // unveiled to defend the hanging bishop d3.
    let items = defenses("3r2k1/8/8/8/8/3B4/3N4/3N2K1 w - - 0 1");
    assert!(items.is_empty());
}

#[test]
fn no_discovered_defenses_in_the_opening_position() {
    assert!(defenses("rnbqkbnr/pppppppp/8/8/8/8/PPPPPPPP/RNBQKBNR w - - 0 1").is_empty());
}

#[test]
fn enumeration_does_not_mutate_the_position() {
    let fen = "3r2k1/8/8/8/8/3B4/3N4/3R2K1 w - - 0 1";
    let pos = Position::from_fen(fen).unwrap();
    let _ = discovered_defense_opportunities(&pos);
    assert_eq!(
        pos.to_fen(),
        fen,
        "discovered-defense enumeration must not mutate the board"
    );
}
