use cvs_bitboard_core::facts::motifs::discovery_opportunities;
use cvs_bitboard_core::facts::{DiscoveryOpportunity, FactCollection, PieceType};
use cvs_bitboard_core::Position;

fn discoveries(fen: &str) -> Vec<DiscoveryOpportunity> {
    let pos = Position::from_fen(fen).unwrap();
    match discovery_opportunities(&pos) {
        FactCollection::Computed { items } => items,
        other => panic!("discoveries should be computed, got {other:?}"),
    }
}

fn find<'a>(items: &'a [DiscoveryOpportunity], uci: &str) -> Option<&'a DiscoveryOpportunity> {
    items.iter().find(|d| d.move_uci == uci)
}

// ── positives ────────────────────────────────────────────────────────────────

#[test]
fn clean_discovered_attack_knight_unveils_rook_onto_bishop() {
    // Nd3-f4 vacates the d-file; rook d1 now attacks the undefended bishop d8, which —
    // unlike a rook or queen — cannot capture the (undefended) rook back down the file.
    let items = discoveries("3b2k1/8/8/8/8/3N4/8/3R2K1 w - - 0 1");
    let d = find(&items, "d3f4").expect("Nf4 should discover the rook onto the bishop");
    assert_eq!(d.kind, "discovered_attack");
    assert_eq!(d.validator, "discovery_validation");
    assert_eq!(d.slider.piece_type, PieceType::Rook);
    assert_eq!(d.slider.square, "d1");
    assert_eq!(d.target.piece_type, PieceType::Bishop);
    assert_eq!(d.target.square, "d8");
    assert!(!d.gives_check);
    assert!(!d.discovered_check);
    assert!(!d.double_check);
    assert_eq!(d.material_gain, 300);
}

#[test]
fn discovered_check_bishop_steps_off_the_rook_file() {
    // Be4-d5 vacates the e-file; rook e1 now checks the king e8.
    let items = discoveries("4k3/8/8/8/4B3/8/8/4R1K1 w - - 0 1");
    let d = find(&items, "e4d5").expect("Bd5 should discover the rook check on e8");
    assert_eq!(d.kind, "discovered_check");
    assert!(d.discovered_check);
    assert!(!d.double_check);
    assert_eq!(d.slider.square, "e1");
    assert_eq!(d.target.piece_type, PieceType::King);
    assert_eq!(d.target.square, "e8");
    assert!(d.gives_check);
    assert_eq!(d.material_gain, 0);
}

#[test]
fn double_check_knight_checks_and_unveils_the_rook_check() {
    // Ne4-d6 checks e8 itself AND unveils rook e1 onto e8 — a double check.
    let items = discoveries("4k3/8/8/8/4N3/8/8/4R1K1 w - - 0 1");
    let d = find(&items, "e4d6").expect("Nd6 should be a double check");
    assert_eq!(d.kind, "double_check");
    assert!(d.discovered_check);
    assert!(d.double_check);
    assert_eq!(d.slider.square, "e1");
    assert_eq!(d.target.piece_type, PieceType::King);
    assert_eq!(d.target.square, "e8");
    assert!(d.gives_check);
    assert_eq!(d.material_gain, 0);
}

#[test]
fn discoverer_checks_mover_checks_while_unveiling_a_winning_rook() {
    // Na4-b6+ checks the black king on d5 (the MOVING piece gives check) AND vacates the
    // a-file so rook a1 wins the undefended rook a8. Distinct from a discovered_check (there
    // the UNVEILED piece checks): here the mover checks and the discovery wins the material.
    // Every knight-check response is a king move that cannot reach a8, so the win is clean.
    let items = discoveries("r7/8/8/3k4/N7/8/8/R3K3 w - - 0 1");
    let d = find(&items, "a4b6").expect("Nb6+ should be a discoverer-checks winning the rook a8");
    assert_eq!(d.kind, "discoverer_checks");
    assert!(d.gives_check, "the mover gives check");
    assert!(!d.discovered_check, "the unveiled rook does not check the king");
    assert!(!d.double_check);
    assert_eq!(d.slider.square, "a1");
    assert_eq!(d.target.piece_type, PieceType::Rook);
    assert_eq!(d.target.square, "a8");
    assert_eq!(d.material_gain, 500);
}

#[test]
fn discovered_attack_winning_an_undefended_knight() {
    // Nd3-e5 vacates the d-file; rook d1 wins the undefended knight d6 (a knight, like a
    // bishop, cannot capture the rook back down the file).
    let items = discoveries("6k1/8/3n4/8/8/3N4/8/3R2K1 w - - 0 1");
    let d = find(&items, "d3e5").expect("Ne5 should discover the rook onto the knight d6");
    assert_eq!(d.kind, "discovered_attack");
    assert_eq!(d.target.piece_type, PieceType::Knight);
    assert_eq!(d.target.square, "d6");
    assert!(!d.gives_check);
    assert_eq!(d.material_gain, 300);
}

#[test]
fn diagonal_discovered_check_by_a_bishop_rear_slider() {
    // Nc4-e5 vacates the a2–g8 diagonal; bishop a2 now checks the king g8.
    let items = discoveries("6k1/8/8/8/2N5/8/B7/6K1 w - - 0 1");
    let d = find(&items, "c4e5").expect("Ne5 should discover the bishop check on g8");
    assert_eq!(d.kind, "discovered_check");
    assert!(d.discovered_check);
    assert!(!d.double_check);
    assert_eq!(d.slider.piece_type, PieceType::Bishop);
    assert_eq!(d.slider.square, "a2");
    assert_eq!(d.target.piece_type, PieceType::King);
    assert_eq!(d.target.square, "g8");
    assert!(d.gives_check);
}

// ── negatives (must NOT fire) ────────────────────────────────────────────────

#[test]
fn rejects_a_mover_sliding_along_its_own_ray() {
    // Rd3-d5 slides along the d-file; the rook stays on the rear rook's ray.
    let items = discoveries("3q2k1/8/8/8/8/3R4/8/3R2K1 w - - 0 1");
    assert!(find(&items, "d3d5").is_none());
}

#[test]
fn rejects_a_second_blocker_on_the_line() {
    // After Nf4, the white pawn d6 still blocks rook d1 from the queen d8.
    let items = discoveries("3q2k1/8/3P4/8/8/3N4/8/3R2K1 w - - 0 1");
    assert!(find(&items, "d3f4").is_none());
}

#[test]
fn rejects_an_unveiled_target_that_is_defended_and_not_dearer() {
    // Rook d1 would see rook d5, but d5 is defended (bishop c6) and equal value.
    let items = discoveries("6k1/8/2b5/3r4/8/3N4/8/3R2K1 w - - 0 1");
    assert!(find(&items, "d3f4").is_none());
}

#[test]
fn rejects_when_there_is_no_rear_slider() {
    // Lone knight hop; no friendly slider behind it, and it just hangs to fxe5.
    let items = discoveries("3q2k1/8/5p2/8/8/3N4/8/6K1 w - - 0 1");
    assert!(find(&items, "d3e5").is_none());
}

#[test]
fn rejects_when_the_rear_piece_is_a_non_slider() {
    // The piece behind on d1 is a knight, not a slider — nothing is unveiled.
    let items = discoveries("3q2k1/8/8/8/8/3N4/8/3N2K1 w - - 0 1");
    assert!(find(&items, "d3f4").is_none());
}

#[test]
fn rejects_an_en_passant_that_unveils_no_winnable_target() {
    let items = discoveries("6k1/8/8/3pP3/8/8/8/3RK3 w - d6 0 1");
    assert!(find(&items, "e5d6").is_none());
}

#[test]
fn rejects_a_discovery_whose_rear_slider_is_only_traded_off() {
    // Knight moves that "unveil" rook d1 onto the d8 rook are moot: Black just plays
    // Rxd1 — an even trade down the now-open file — neutralizing the threat at no cost.
    // In particular d3b2 / d3f2 (where the knight then GUARDS d1, so the old SEE>0 gate
    // wrongly passed) must NOT fire; nothing should claim the tradeable d8 rook.
    let items = discoveries("3r2k1/8/8/8/8/3N4/8/3R2K1 w - - 0 1");
    assert!(find(&items, "d3b2").is_none());
    assert!(find(&items, "d3f2").is_none());
    assert!(
        items.iter().all(|d| d.target.square != "d8"),
        "no discovery should claim the tradeable d8 rook"
    );
}

#[test]
fn no_discoveries_in_the_opening_position() {
    assert!(discoveries("rnbqkbnr/pppppppp/8/8/8/8/PPPPPPPP/RNBQKBNR w KQkq - 0 1").is_empty());
}

#[test]
fn enumeration_does_not_mutate_the_position() {
    let fen = "3q2k1/8/8/8/8/3N4/8/3R2K1 w - - 0 1";
    let pos = Position::from_fen(fen).unwrap();
    let _ = discovery_opportunities(&pos);
    assert_eq!(pos.to_fen(), fen, "discovery enumeration must not mutate the board");
}
