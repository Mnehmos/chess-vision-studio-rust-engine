use cvs_bitboard_core::facts::motifs::xray_defense_opportunities;
use cvs_bitboard_core::facts::{FactCollection, PieceType, XRayDefenseOpportunity};
use cvs_bitboard_core::Position;

fn xr(fen: &str) -> Vec<XRayDefenseOpportunity> {
    let pos = Position::from_fen(fen).unwrap();
    match xray_defense_opportunities(&pos) {
        FactCollection::Computed { items } => items,
        other => panic!("xray defenses should be computed, got {other:?}"),
    }
}

// ── Positives ────────────────────────────────────────────────────────────────

#[test]
fn validates_a_rook_defending_a_knight_through_an_enemy_rook() {
    // White to move: Ra1-d1 defends the white knight on d6 THROUGH the black rook on d5.
    // A naive one-square defender count of d6 is blocked by the black rook, so d6 looks
    // undefended; but if Black plays Rd5xd6, the rook vacates the intervening square and
    // Rd1xd6 recaptures down the cleared file. The knight is held only via the x-ray.
    let items = xr("7k/8/3N4/3r4/8/8/8/R3K3 w - - 0 1");
    let x = items
        .iter()
        .find(|x| x.move_uci == "a1d1")
        .expect("Rd1 x-ray defense of the knight should be validated");
    assert_eq!(x.kind, "xray_defense");
    assert_eq!(x.validator, "xray_defense_validation");
    assert_eq!(x.xrayer.piece_type, PieceType::Rook);
    assert_eq!(x.xrayer.square, "d1");
    assert_eq!(x.front_enemy.piece_type, PieceType::Rook);
    assert_eq!(x.front_enemy.square, "d5");
    assert_eq!(x.defended.piece_type, PieceType::Knight);
    assert_eq!(x.defended.square, "d6");
    assert!(!x.gives_check);
    assert_eq!(x.material_gain, 320);
    assert_eq!(x.ray, vec!["d2", "d3", "d4", "d5"]);
}

#[test]
fn validates_a_bishop_defending_a_bishop_through_an_enemy_bishop() {
    // White to move: Bc1-a3 defends the white bishop on e7 THROUGH the black bishop on d6
    // along the a3-f8 diagonal. Bd6xe7 Ba3xe7 holds the bishop through the cleared diagonal.
    let items = xr("7k/4B3/3b4/8/8/8/8/2B3K1 w - - 0 1");
    let x = items
        .iter()
        .find(|x| x.move_uci == "c1a3")
        .expect("Ba3 x-ray defense of the bishop should be validated");
    assert_eq!(x.xrayer.piece_type, PieceType::Bishop);
    assert_eq!(x.xrayer.square, "a3");
    assert_eq!(x.front_enemy.piece_type, PieceType::Bishop);
    assert_eq!(x.front_enemy.square, "d6");
    assert_eq!(x.defended.piece_type, PieceType::Bishop);
    assert_eq!(x.defended.square, "e7");
    assert!(!x.gives_check);
    assert_eq!(x.material_gain, 330);
    assert_eq!(x.ray, vec!["b4", "c5", "d6"]);
}

#[test]
fn validates_a_queen_defending_a_rook_through_an_enemy_queen() {
    // White to move: Qa1-a4 defends the white rook on g4 THROUGH the black queen on e4 on
    // the 4th rank. The queen's landing square a4 is protected by the pawn on b3, so the
    // queen does not simply hang. Qe4xg4 Qa4xg4 holds the rook through the cleared rank.
    let items = xr("2k5/8/8/8/4q1R1/1P6/7K/Q7 w - - 0 1");
    let x = items
        .iter()
        .find(|x| x.move_uci == "a1a4")
        .expect("Qa4 x-ray defense of the rook should be validated");
    assert_eq!(x.xrayer.piece_type, PieceType::Queen);
    assert_eq!(x.xrayer.square, "a4");
    assert_eq!(x.front_enemy.piece_type, PieceType::Queen);
    assert_eq!(x.front_enemy.square, "e4");
    assert_eq!(x.defended.piece_type, PieceType::Rook);
    assert_eq!(x.defended.square, "g4");
    assert!(!x.gives_check);
    assert_eq!(x.material_gain, 500);
    assert_eq!(x.ray, vec!["b4", "c4", "d4", "e4", "f4"]);
}

// ── Negatives ────────────────────────────────────────────────────────────────

#[test]
fn does_not_report_plain_over_protection_through_a_friendly_front() {
    // The piece between the rook and the defended knight is OUR OWN pawn on d4, not an
    // enemy. A friendly front is normal stacked defense, not an x-ray (fails Gc: the front
    // must be an enemy piece). The black rook on d7 attacks the knight, so there IS a
    // target, but no enemy sits on the defending line.
    let items = xr("7k/3r4/3N4/8/3P4/8/8/R3K3 w - - 0 1");
    assert!(
        items.iter().all(|x| x.move_uci != "a1d1"),
        "a friendly front is over-protection, not an x-ray defense, got {items:?}"
    );
}

#[test]
fn rejects_a_defense_that_an_independent_defender_already_holds() {
    // The knight on d6 is independently defended by the white pawn on c5, so removing the
    // rook still holds it: Rd5xd6 c5xd6 loses the black rook. The x-ray is not load-bearing
    // (guard iii: without-the-xrayer SEE stays <= 0), so it is plain defense, not an x-ray.
    let items = xr("7k/8/3N4/2Pr4/8/8/8/R3K3 w - - 0 1");
    assert!(
        items.iter().all(|x| x.move_uci != "a1d1"),
        "an independently held piece is not saved BY the x-ray, got {items:?}"
    );
}

#[test]
fn rejects_a_pinned_xrayer() {
    // White is in check from the black rook on a8; Bc1-a3 interposes on the a-file and is
    // pinned to the white king on a1. see() ignores the pin and says Be7 is held THROUGH
    // Bd6, but the bishop can never legally recapture (leaving the a-file exposes the king).
    // The legality guard (Gd/G7) must reject it.
    let items = xr("r6k/4B3/3b4/8/8/8/8/K1B5 w - - 0 1");
    assert!(
        items.iter().all(|x| x.move_uci != "c1a3"),
        "a pinned x-rayer cannot execute its recapture, got {items:?}"
    );
}

#[test]
fn rejects_when_the_defended_piece_is_not_under_attack() {
    // The black pawn on d5 is the enemy front, but a pawn does not attack up the file, so
    // the knight on d6 is not threatened at all. A through-line alignment with no threat is
    // not a defense (guard i: G must have an enemy attacker).
    let items = xr("7k/8/3N4/3p4/8/8/8/R3K3 w - - 0 1");
    assert!(
        items.iter().all(|x| x.move_uci != "a1d1"),
        "nothing is defended when the piece is not attacked, got {items:?}"
    );
}

#[test]
fn rejects_an_xrayer_that_simply_hangs() {
    // Ra1-d1 lands on a square won by Black: the black bishop on a4 and the black rook on
    // d5 both bear on d1 and the lone white king cannot hold it (Ba4xd1 wins the rook, the
    // king may not recapture into the rook's defense). The defending slider is simply lost
    // (guard Ga).
    let items = xr("7k/8/3N4/3r4/b7/8/8/R3K3 w - - 0 1");
    assert!(
        items.iter().all(|x| x.move_uci != "a1d1"),
        "an x-rayer that immediately hangs is not a defense, got {items:?}"
    );
}

#[test]
fn rejects_when_the_target_is_lost_regardless_of_the_xray() {
    // The knight on d6 is also attacked by a black knight on e4. Ne4xd6 wins the knight
    // whether or not the rook is behind, because the front rook still blocks the recapture
    // when a DIFFERENT piece captures. best_see_capture(enemy, d6) > 0 (guard ii): the piece
    // is lost anyway, so there is no save to report.
    let items = xr("7k/8/3N4/3r4/4n3/8/8/R3K3 w - - 0 1");
    assert!(
        items.iter().all(|x| x.move_uci != "a1d1"),
        "a piece lost regardless of the x-ray is not saved by it, got {items:?}"
    );
}

// ── Invariants ───────────────────────────────────────────────────────────────

#[test]
fn no_xray_defenses_in_the_opening_position() {
    assert!(
        xr("rnbqkbnr/pppppppp/8/8/8/8/PPPPPPPP/RNBQKBNR w KQkq - 0 1").is_empty(),
        "the start position has no x-ray defenses"
    );
}

#[test]
fn enumeration_does_not_mutate_the_position() {
    let fen = "7k/8/3N4/3r4/8/8/8/R3K3 w - - 0 1";
    let pos = Position::from_fen(fen).unwrap();
    let _ = xray_defense_opportunities(&pos);
    assert_eq!(
        pos.to_fen(),
        fen,
        "x-ray-defense enumeration must not mutate the board"
    );
}

#[test]
fn emitted_opportunities_report_a_positive_material_gain() {
    // Every reported x-ray defense must carry a proven positive saved-material gain.
    for fen in [
        "7k/8/3N4/3r4/8/8/8/R3K3 w - - 0 1",
        "7k/4B3/3b4/8/8/8/8/2B3K1 w - - 0 1",
        "2k5/8/8/8/4q1R1/1P6/7K/Q7 w - - 0 1",
    ] {
        for x in xr(fen) {
            assert!(
                x.material_gain > 0,
                "x-ray defense on {fen} reported non-positive gain: {x:?}"
            );
        }
    }
}
