//! Adversarial FEN battery for the INTERFERENCE detector (ChessTempo "Interference").
//!
//! A MOVE detector: a move places a piece on a square S strictly between an enemy SLIDER D
//! and an enemy target P that D defends along that ray, severing the defense so P is winnable.
//! Soundness is a worst-case over ALL enemy replies — in particular a slider recapturing our
//! interposer on S usually re-defends P (it lands back on the D-P line), so a genuine
//! interference win is rare: the defender must be unable to recapture (pinned) and the target
//! unable to flee. material_gain uses SEE_VALUE (knight 320, bishop 330, rook 500).

use cvs_bitboard_core::facts::motifs::interference_opportunities;
use cvs_bitboard_core::facts::{FactCollection, InterferenceOpportunity, PieceType};
use cvs_bitboard_core::Position;

fn intf(fen: &str) -> Vec<InterferenceOpportunity> {
    let pos = Position::from_fen(fen).unwrap();
    match interference_opportunities(&pos) {
        FactCollection::Computed { items } => items,
        other => panic!("interference should be computed, got {other:?}"),
    }
}

fn by_move<'a>(it: &'a [InterferenceOpportunity], uci: &str) -> Option<&'a InterferenceOpportunity> {
    it.iter().find(|o| o.move_uci == uci)
}

// ── positives (must fire) ─────────────────────────────────────────────────────

#[test]
fn knight_interposes_to_cut_a_pinned_bishops_defense() {
    // Black Bf6 defends Na1 along the a1-h8 diagonal, but Bf6 is PINNED to Kf8 by Rf1 (can't
    // recapture) and Na1 is TRAPPED (Bd1 covers b3+c2). White Ne2-c3 interposes on the
    // diagonal; the defense is severed and Rb1xa1 wins the knight (320). Ne2-d4 does the same.
    let items = intf("5k2/8/5b2/8/8/8/4N3/nR1B1RK1 w - - 0 1");
    let o = by_move(&items, "e2c3").expect("Ne2-c3 interposes to cut Bf6's defense of Na1");
    assert_eq!(o.kind, "interference");
    assert_eq!(o.validator, "interference_validation");
    assert_eq!(o.interposer.piece_type, PieceType::Knight);
    assert_eq!(o.interposer.square, "c3");
    assert_eq!(o.cut_defender.piece_type, PieceType::Bishop);
    assert_eq!(o.cut_defender.square, "f6");
    assert_eq!(o.target.piece_type, PieceType::Knight);
    assert_eq!(o.target.square, "a1");
    assert!(!o.gives_check);
    assert_eq!(o.material_gain, 320);
    // the alternative interposition on the same diagonal also fires
    assert!(by_move(&items, "e2d4").is_some(), "Ne2-d4 is an equally valid interposition");
}

#[test]
fn bishop_interposes_on_a_long_diagonal_defense() {
    // A real game position: White Bc1-b2 interposes on b2, cutting black Bf6's defense of the
    // cornered Na1 down the long diagonal; against best defense White nets +150.
    let items = intf("r1b1k2r/p1pp1p1p/1p2pb1P/6p1/NqB2PPn/1P2P2K/P2PN3/nRBQ3R w - - 7 22");
    let o = by_move(&items, "c1b2").expect("Bc1-b2 interposes to cut Bf6's defense of Na1");
    assert_eq!(o.interposer.piece_type, PieceType::Bishop);
    assert_eq!(o.interposer.square, "b2");
    assert_eq!(o.cut_defender.square, "f6");
    assert_eq!(o.target.square, "a1");
    assert_eq!(o.material_gain, 150);
}

// ── adversarial negatives (must produce ZERO ops) ─────────────────────────────

#[test]
fn rejects_when_the_defender_recaptures_on_S_and_redefends() {
    // LOAD-BEARING: black Bb8 defends Ne5 along b8-e5; White Nf5-d6 interposes on d6 (defended
    // by Pc5). But d6 is on the diagonal, so black Bxd6 recaptures and the bishop — now on d6 —
    // STILL defends e5. A naive "cut the line" detector fires; the all-replies worst-case must
    // suppress it (this is interference's analogue of attack-defender's AN1).
    assert!(
        intf("1b2k3/8/8/2P1nN2/8/8/8/4R1K1 w - - 0 1").is_empty(),
        "a defender that recaptures on S and re-defends the target refutes the interference"
    );
}

#[test]
fn rejects_when_the_interposer_simply_hangs() {
    // Same shape but S (d6) is undefended: black Bxd6 wins the interposer outright.
    assert!(intf("1b2k3/8/8/4nN2/8/8/8/4R1K1 w - - 0 1").is_empty());
}

#[test]
fn rejects_when_the_target_can_flee() {
    // Same shape but the target is NOT pinned (black king on a8), so after the cut the knight
    // simply moves to safety on the enemy's reply — no material won.
    assert!(intf("kb6/8/8/2P1nN2/8/8/8/4R1K1 w - - 0 1").is_empty());
}

#[test]
fn rejects_when_the_target_has_a_second_defender() {
    // A black knight on c4 also guards e5 from off the diagonal, so cutting the bishop's line
    // leaves the target defended (SEE counts the remaining defender).
    assert!(intf("1b2k3/8/8/2P1nN2/2n5/8/8/4R1K1 w - - 0 1").is_empty());
}

#[test]
fn no_interference_in_the_opening_position() {
    assert!(intf("rnbqkbnr/pppppppp/8/8/8/8/PPPPPPPP/RNBQKBNR w KQkq - 0 1").is_empty());
}

// ── invariant ─────────────────────────────────────────────────────────────────

#[test]
fn enumeration_does_not_mutate_the_position() {
    let fen = "5k2/8/5b2/8/8/8/4N3/nR1B1RK1 w - - 0 1";
    let pos = Position::from_fen(fen).unwrap();
    let _ = interference_opportunities(&pos);
    assert_eq!(pos.to_fen(), fen, "interference enumeration must not mutate the board");
}
