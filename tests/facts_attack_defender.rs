//! Adversarial FEN battery for the ATTACKING-THE-DEFENDER detector (ChessTempo).
//!
//! A MOVE detector: a move whose moved piece newly attacks an enemy piece D that is the
//! SOLE defender of one or more enemy charges P. D must move or be lost, and — against
//! EVERY legal enemy reply — we still win material (win the standing/relocated defender,
//! or a charge the eviction abandons). Disjoint from remove_guard (that CAPTURES D).
//!
//! material_gain uses SEE_VALUE (knight 320, bishop 330, rook 500) — the best_see_capture
//! scale, the least-bad outcome over the enemy's best reply. The load-bearing negative is
//! AN1: a naive D-escape-only detector fires it; the sound all-replies detector must not
//! (the guarded piece counter-captures our attacker with check).

use cvs_bitboard_core::facts::motifs::attack_defender_opportunities;
use cvs_bitboard_core::facts::{AttackDefenderOpportunity, FactCollection, PieceType};
use cvs_bitboard_core::Position;

fn ad(fen: &str) -> Vec<AttackDefenderOpportunity> {
    let pos = Position::from_fen(fen).unwrap();
    match attack_defender_opportunities(&pos) {
        FactCollection::Computed { items } => items,
        other => panic!("attack-defender should be computed, got {other:?}"),
    }
}

fn by_move<'a>(it: &'a [AttackDefenderOpportunity], uci: &str) -> Option<&'a AttackDefenderOpportunity> {
    it.iter().find(|o| o.move_uci == uci)
}

fn target_squares(o: &AttackDefenderOpportunity) -> Vec<&str> {
    o.targets.iter().map(|t| t.square.as_str()).collect()
}

// ── positives (must fire) ─────────────────────────────────────────────────────

#[test]
fn queen_chases_a_knight_guard() {
    // AP1: ...Qc2 attacks the undefended Nb3, the sole guard of Bd2. If the knight moves,
    // ...Qxd2 wins the bishop (330); if it stays, ...Qxb3 wins the knight (320). Worst 320.
    let items = ad("8/2q1k3/8/8/8/1N6/3B2K1/8 b - - 0 1");
    let o = by_move(&items, "c7c2").expect("Qc2 attacks the knight-guard Nb3");
    assert_eq!(o.kind, "attacking_the_defender");
    assert_eq!(o.validator, "attack_defender_validation");
    assert_eq!(o.attacked_defender.piece_type, PieceType::Knight);
    assert_eq!(o.attacked_defender.square, "b3");
    assert_eq!(target_squares(o), vec!["d2"]);
    assert!(!o.gives_check);
    assert_eq!(o.material_gain, 320);
}

#[test]
fn white_rook_chases_an_overworked_queen() {
    // AP2: Re6 attacks the black Qe5, the sole guard of Ne7. The queen must flee, and
    // Rxe7 collects the knight (320). White exploiting a black defender.
    let items = ad("1k6/4nK2/5R2/4q3/8/8/8/8 w - - 0 1");
    let o = by_move(&items, "f6e6").expect("Re6 attacks the queen-guard Qe5");
    assert_eq!(o.attacked_defender.piece_type, PieceType::Queen);
    assert_eq!(o.attacked_defender.square, "e5");
    assert_eq!(target_squares(o), vec!["e7"]);
    assert_eq!(o.material_gain, 320);
}

#[test]
fn a_king_move_can_attack_the_defender() {
    // AP3: ...Kf2 attacks the Ne1 (sole guard of Bf3); the knight must move and ...Kxf3
    // wins the bishop. Proves non-piece movers work.
    let items = ad("8/8/8/2K5/8/5B1P/8/4N1k1 b - - 0 1");
    let o = by_move(&items, "g1f2").expect("Kf2 attacks the knight-guard Ne1");
    assert_eq!(o.attacked_defender.piece_type, PieceType::Knight);
    assert_eq!(o.attacked_defender.square, "e1");
    assert_eq!(target_squares(o), vec!["f3"]);
}

// ── adversarial negatives (must produce ZERO ops) ─────────────────────────────

#[test]
fn rejects_when_the_charge_counter_captures_our_attacker() {
    // AN1 (LOAD-BEARING): 1.Bc4 attacks Ne6 (sole guard of ... nothing here) — the real
    // trap is the guarded rook Rd8 replying Rxd1, grabbing our undefended rook. A naive
    // D-escape-only detector FIRES this; the all-replies worst-case must suppress it.
    assert!(
        ad("3r2k1/8/4n3/8/8/8/8/3R1BK1 w - - 0 1").is_empty(),
        "the enemy counter-capture of our attacker refutes the eviction"
    );
}

#[test]
fn rejects_when_the_guard_interposes_defended_by_the_charge() {
    // AN2: 1.Bc4 Nh6 blocks the h-file; Rh8 defends h6 → nothing won.
    assert!(ad("7r/5n2/8/k7/8/8/4B3/6KR w - - 0 1").is_empty());
}

#[test]
fn rejects_when_our_chasing_piece_hangs() {
    // AN3: 1.Bc4 hangs to ...bxc4.
    assert!(ad("3r2k1/8/4n3/1p6/8/8/8/3R1BK1 w - - 0 1").is_empty());
}

#[test]
fn rejects_when_the_charge_is_already_winnable() {
    // AN4: doubled rooks already win Rd8 pre-move → causality guard.
    assert!(ad("3r2k1/8/4n3/8/8/8/3R4/3R1BK1 w - - 0 1").is_empty());
}

#[test]
fn rejects_when_the_charge_has_a_second_defender() {
    // AN5: Rd8 is guarded by Ne6 AND Re8 → the knight is not its sole guard.
    assert!(ad("3rr1k1/8/4n3/8/8/8/8/3R1BK1 w - - 0 1").is_empty());
}

#[test]
fn rejects_when_the_defender_is_not_winnable_in_place() {
    // AN7: the black king defends the attacked knight-guard, so it is not won in place.
    assert!(ad("4nk2/8/8/8/8/8/2B5/3R2K1 w - - 0 1").is_empty());
}

#[test]
fn rejects_when_the_only_sole_defended_piece_is_a_pawn() {
    // AN8: the sole-defended piece is a pawn → filtered (pawns excluded as charges).
    assert!(ad("3p2k1/8/4n3/8/8/8/8/3R1BK1 w - - 0 1").is_empty());
}

#[test]
fn rejects_when_the_defender_was_already_attacked() {
    // AN9: the defender is already attacked before the move → causality (not caused by mv).
    assert!(ad("3r2k1/8/4n3/8/2B5/8/8/3R2K1 w - - 0 1").is_empty());
}

#[test]
fn no_ops_in_the_opening_position() {
    assert!(ad("rnbqkbnr/pppppppp/8/8/8/8/PPPPPPPP/RNBQKBNR w KQkq - 0 1").is_empty());
}

// ── invariant ─────────────────────────────────────────────────────────────────

#[test]
fn enumeration_does_not_mutate_the_position() {
    let fen = "8/2q1k3/8/8/8/1N6/3B2K1/8 b - - 0 1";
    let pos = Position::from_fen(fen).unwrap();
    let _ = attack_defender_opportunities(&pos);
    assert_eq!(pos.to_fen(), fen, "attack-defender enumeration must not mutate the board");
}
