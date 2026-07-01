use cvs_bitboard_core::facts::motifs::double_attack_opportunities;
use cvs_bitboard_core::facts::{DoubleAttackOpportunity, FactCollection, PieceType};
use cvs_bitboard_core::Position;

fn double_attacks(fen: &str) -> Vec<DoubleAttackOpportunity> {
    let pos = Position::from_fen(fen).unwrap();
    match double_attack_opportunities(&pos) {
        FactCollection::Computed { items } => items,
        other => panic!("double attacks should be computed, got {other:?}"),
    }
}

fn find<'a>(items: &'a [DoubleAttackOpportunity], uci: &str) -> Option<&'a DoubleAttackOpportunity> {
    items.iter().find(|d| d.move_uci == uci)
}

// ── positives (must fire double_attack) ──────────────────────────────────────

#[test]
fn knight_capture_of_a_guard_double_attacks_queen_and_frees_a_rook() {
    // Nc5xb7 captures the bishop b7 (the only defender of rook a8). From b7 the knight
    // attacks the undefended queen d6 (threat A, 900). The distinct rook a1 then wins the
    // now-undefended a8 rook (threat B, 500). The enemy saves the dearer -> we take
    // min(900, 500) = 500.
    let items = double_attacks("r3k3/1b6/3q4/2N5/8/8/8/R3K3 w - - 0 1");
    let d = find(&items, "c5b7").expect("Nxb7 should double-attack the queen and free the rook");
    assert_eq!(d.kind, "double_attack");
    assert_eq!(d.validator, "double_attack_validation");
    assert_eq!(d.mover.piece_type, PieceType::Knight);
    assert_eq!(d.mover.square, "b7");
    assert_eq!(d.second_attacker.piece_type, PieceType::Rook);
    assert_eq!(d.second_attacker.square, "a1");
    assert_eq!(d.target_a.piece_type, PieceType::Queen);
    assert_eq!(d.target_a.square, "d6");
    assert_eq!(d.target_b.piece_type, PieceType::Rook);
    assert_eq!(d.target_b.square, "a8");
    assert!(!d.gives_check);
    assert_eq!(d.material_gain, 500);
}

#[test]
fn min_of_the_two_prongs_is_the_lesser_target() {
    // Same geometry, but threat A is only a bishop (300) instead of a queen. The enemy
    // saves the dearer rook -> we take min(300, 500) = 300.
    let items = double_attacks("r3k3/1b6/3b4/2N5/8/8/8/R3K3 w - - 0 1");
    let d = find(&items, "c5b7").expect("Nxb7 should double-attack the bishop and free the rook");
    assert_eq!(d.mover.piece_type, PieceType::Knight);
    assert_eq!(d.target_a.piece_type, PieceType::Bishop);
    assert_eq!(d.target_a.square, "d6");
    assert_eq!(d.target_b.piece_type, PieceType::Rook);
    assert_eq!(d.target_b.square, "a8");
    assert_eq!(d.material_gain, 300);
}

#[test]
fn rook_mover_double_attacks_along_the_rank_and_frees_a_rook() {
    // Rb2xb7 captures the bishop b7 (defender of rook a8). From b7 the rook attacks the
    // undefended knight e7 along the clear 7th rank (threat A, 300). The distinct rook a1
    // wins the now-undefended a8 rook (threat B, 500). min(300, 500) = 300. Confirms the
    // mover need not be a knight, and the two threats issue from two different pieces.
    let items = double_attacks("r6k/1b2n3/8/8/8/8/1R6/R3K3 w - - 0 1");
    let d = find(&items, "b2b7").expect("Rxb7 should double-attack the knight and free the rook");
    assert_eq!(d.mover.piece_type, PieceType::Rook);
    assert_eq!(d.mover.square, "b7");
    assert_eq!(d.second_attacker.piece_type, PieceType::Rook);
    assert_eq!(d.second_attacker.square, "a1");
    assert_ne!(
        d.mover.square, d.second_attacker.square,
        "double attack is two DIFFERENT pieces"
    );
    assert_eq!(d.target_a.piece_type, PieceType::Knight);
    assert_eq!(d.target_a.square, "e7");
    assert_eq!(d.target_b.piece_type, PieceType::Rook);
    assert_eq!(d.target_b.square, "a8");
    assert!(!d.gives_check);
    assert_eq!(d.material_gain, 300);
}

// ── negatives (must NOT fire) ────────────────────────────────────────────────

#[test]
fn rejects_a_pure_fork_one_piece_two_targets() {
    // The knight on e4 already attacks both rooks (d6 and f6) — any of its moves that keeps
    // hitting two targets is a fork. A single moved piece hitting two targets is the fork
    // detector's fact: both threats come from mv.to, so the distinct-piece guard (G5)
    // rejects any double_attack claim here.
    let items = double_attacks("4k3/8/3r1r2/8/4N3/8/8/4K3 w - - 0 1");
    assert!(
        items.is_empty(),
        "a single knight hitting two rooks is a fork, not a double attack: {items:?}"
    );
}

#[test]
fn rejects_a_discovery_second_attacker() {
    // Nd3-f4 vacates the d-file so rook d1 sees the d8 rook only via the mv.from vacancy —
    // that is the discovery detector's fact. G6 (double_attack_is_discovery) rejects it.
    let items = double_attacks("3r2k1/8/8/8/8/3N4/8/3R2K1 w - - 0 1");
    assert!(find(&items, "d3f4").is_none(), "discovery geometry must not fire double_attack");
    assert!(items.is_empty(), "no double attack in a pure discovery position: {items:?}");
}

#[test]
fn rejects_a_second_threat_that_already_existed() {
    // Rook d2 already winnably attacks rook d7 BEFORE any knight move (open d-file, no
    // guard), so no move CREATES threat B. Causality guard (G3) rejects.
    let items = double_attacks("4k3/3r4/8/8/4N3/8/3R4/4K3 w - - 0 1");
    assert!(items.is_empty(), "a pre-existing second threat is not created by the move: {items:?}");
}

#[test]
fn rejects_a_move_whose_mover_simply_hangs() {
    // Nd2-c4 "attacks" the queen d6 but the knight on c4 is captured for gain (bxc4 /
    // Nxc4). G1 (forker_capturable_for_gain) rejects: the enemy just takes the mover.
    let items = double_attacks("4k3/8/3q4/8/8/2n5/3N4/3RK3 w - - 0 1");
    assert!(find(&items, "d2c4").is_none(), "a hung mover cannot deliver threat A");
    assert!(items.is_empty(), "no sound double attack when the mover hangs: {items:?}");
}

#[test]
fn rejects_a_defended_equal_value_second_target() {
    // The rook d2 "attacks" rook d5, but d5 is defended (bishop c6) and equal value, so
    // capturing it is SEE <= 0. G4 (see(after_us, q -> t) > 0) rejects.
    let items = double_attacks("4k3/8/2b5/3r4/8/3N4/3R4/4K3 w - - 0 1");
    assert!(items.is_empty(), "a defended equal-value second target is not winnable: {items:?}");
}

#[test]
fn no_double_attacks_in_the_opening_position() {
    assert!(double_attacks("rnbqkbnr/pppppppp/8/8/8/8/PPPPPPPP/RNBQKBNR w KQkq - 0 1").is_empty());
}

#[test]
fn rejects_a_pinned_mover_threat() {
    // Bf1-e2 interposes against a check (Qd2 on the 2nd rank); the bishop is then pinned to
    // its own king and can NEVER capture c4, so its "threat A" is not real. A double attack
    // must not be claimed (regression for the pinned-mover false positive found by fuzzing).
    let items = double_attacks("r3kbnr/1p1bp1pp/pQ1P4/2p2p2/2p1PP2/6PP/PP1q1K2/R1B2BNR w kq - 0 14");
    assert!(find(&items, "f1e2").is_none(), "a pinned mover has no real threat A");
}

// ── determinism / purity ─────────────────────────────────────────────────────

#[test]
fn enumeration_does_not_mutate_the_position() {
    let fen = "r3k3/1b6/3q4/2N5/8/8/8/R3K3 w - - 0 1";
    let pos = Position::from_fen(fen).unwrap();
    let _ = double_attack_opportunities(&pos);
    assert_eq!(pos.to_fen(), fen, "double-attack enumeration must not mutate the board");
}

#[test]
fn enumeration_is_deterministic() {
    let fen = "r3k3/1b6/3q4/2N5/8/8/8/R3K3 w - - 0 1";
    let a = double_attacks(fen);
    let b = double_attacks(fen);
    assert_eq!(a, b, "double-attack enumeration must be deterministic");
}
