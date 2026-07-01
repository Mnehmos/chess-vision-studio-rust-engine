use cvs_bitboard_core::facts::motifs::desperado_opportunities;
use cvs_bitboard_core::facts::{DesperadoOpportunity, FactCollection, PieceType};
use cvs_bitboard_core::Position;

fn desperado(fen: &str) -> Vec<DesperadoOpportunity> {
    let pos = Position::from_fen(fen).unwrap();
    match desperado_opportunities(&pos) {
        FactCollection::Computed { items } => items,
        other => panic!("desperado should be computed, got {other:?}"),
    }
}

fn at<'a>(it: &'a [DesperadoOpportunity], sq: &str) -> Option<&'a DesperadoOpportunity> {
    it.iter().find(|d| d.piece.square == sq)
}

// ── positives (must fire) ────────────────────────────────────────────────────

#[test]
fn knight_desperado_grabs_the_queen_before_dying() {
    // Black Na1 is doomed: Bb2 attacks it; both hops are covered — b3 by the a2 pawn,
    // c2 by the d2 king. Instead of dying for nothing it plays Nxc2 grabbing the queen;
    // Kxc2 recaptures the knight, so SEE = 900 - 320 = 580.
    let items = desperado("k7/8/8/8/8/8/PBQK4/n7 b - - 0 1");
    let d = at(&items, "a1").expect("the a1 knight should be a desperado");
    assert_eq!(d.kind, "desperado");
    assert_eq!(d.validator, "desperado_validation");
    assert_eq!(d.piece.piece_type, PieceType::Knight);
    assert_eq!(d.move_uci, "a1c2");
    assert_eq!(d.captured_victim.piece_type, PieceType::Queen);
    assert_eq!(d.captured_victim.square, "c2");
    assert_eq!(d.material_gain, 580);
}

#[test]
fn bishop_desperado_grabs_the_queen_on_the_rim() {
    // Black Bh4 is doomed by the g3-square control: its only moves Bxg3 and Bxg5 both
    // relocate onto squares White re-wins. Here a queen sits on g3 (guarded by the h2
    // pawn), so Bxg3 grabs it — SEE = 900 - 330 = 570 — beating dying for nothing.
    let items = desperado("6k1/8/8/6P1/5P1b/6Q1/7P/6K1 b - - 0 1");
    let d = at(&items, "h4").expect("the h4 bishop should be a desperado");
    assert_eq!(d.piece.piece_type, PieceType::Bishop);
    assert_eq!(d.move_uci, "h4g3");
    assert_eq!(d.captured_victim.piece_type, PieceType::Queen);
    assert_eq!(d.captured_victim.square, "g3");
    assert_eq!(d.material_gain, 570);
}

#[test]
fn rook_desperado_grabs_the_queen_from_the_corner() {
    // Black Ra8 is boxed: its own Nb8 blocks b8, and White Qa7 attacks it from a7. Every
    // relocation loses the rook, so it plays Rxa7 grabbing the queen; the b5 knight
    // recaptures, so SEE = 900 - 500 = 400. This is the highest-victim case and confirms
    // the detector reports the queen grab.
    let items = desperado("rn2k3/Q7/8/1N6/8/8/8/7K b - - 0 1");
    let d = at(&items, "a8").expect("the a8 rook should be a desperado");
    assert_eq!(d.piece.piece_type, PieceType::Rook);
    assert_eq!(d.move_uci, "a8a7");
    assert_eq!(d.captured_victim.piece_type, PieceType::Queen);
    assert_eq!(d.captured_victim.square, "a7");
    assert_eq!(d.material_gain, 400);
}

// ── negatives (must NOT report a desperado) ──────────────────────────────────

#[test]
fn not_a_desperado_when_the_piece_has_a_safe_retreat() {
    // Black Ne4 is attacked by Bh1 but has safe hops, so it is not doomed — nothing to
    // "desperado" out of.
    assert!(desperado("7k/8/8/8/4n3/8/8/6KB b - - 0 1").is_empty());
}

#[test]
fn not_a_desperado_when_adequately_defended() {
    // Black Ne6 is attacked by Re2 but defended by the f7 pawn, so it can simply sit —
    // not doomed, not a desperado.
    assert!(desperado("6k1/5p2/4n3/8/8/8/4R3/6K1 b - - 0 1").is_empty());
}

#[test]
fn not_a_desperado_when_doomed_but_nothing_to_grab() {
    // Black Na1 is genuinely doomed (Bb2 attacks; b3/c2 both covered) but every move is
    // quiet — there is no enemy piece to capture on the way out. Dying for nothing is a
    // trap, not a desperado.
    assert!(desperado("k7/8/8/8/8/8/PB1K4/n7 b - - 0 1").is_empty());
}

#[test]
fn not_a_desperado_when_the_grab_is_refuted_by_see() {
    // Black Na1 is doomed and CAN play Nxc2, but the c2 pawn is guarded by the d2 king, so
    // Nxc2 nets 100 - 320 < 0. The SEE gate (not a raw attacker count) suppresses the grab.
    assert!(desperado("k7/8/8/8/8/8/PBPK4/n7 b - - 0 1").is_empty());
}

#[test]
fn not_a_desperado_when_our_king_is_in_check() {
    // Black is in check (Ra1 down the a-file at Ka8), so the enemy-reply probe is
    // unavailable and even a real desperado is suppressed (documented false-negative).
    assert!(desperado("k7/8/8/8/8/8/1r6/R5K1 b - - 0 1").is_empty());
}

#[test]
fn not_a_desperado_when_the_grab_is_illegal_from_a_pin() {
    // Black Nd6 is doomed (the c5 pawn attacks it) and geometrically reaches the white
    // queen on f5, but it is absolutely pinned to Kd8 by Rd1 — every knight move is
    // illegal, so generate_legal drops the grab and no fact fires.
    assert!(desperado("3k4/8/3n4/2P2Q2/8/8/8/3R2K1 b - - 0 1").is_empty());
}

#[test]
fn never_reports_a_king_or_pawn_as_the_desperado() {
    // The detector iterates only N/B/R/Q of our pieces — never a king or pawn.
    for fen in [
        "k7/8/8/8/8/8/PBQK4/n7 b - - 0 1",
        "6k1/8/8/6P1/5P1b/6Q1/7P/6K1 b - - 0 1",
        "rn2k3/Q7/8/1N6/8/8/8/7K b - - 0 1",
    ] {
        for d in desperado(fen) {
            assert_ne!(d.piece.piece_type, PieceType::King);
            assert_ne!(d.piece.piece_type, PieceType::Pawn);
        }
    }
}

#[test]
fn no_desperados_in_the_opening_position() {
    assert!(desperado("rnbqkbnr/pppppppp/8/8/8/8/PPPPPPPP/RNBQKBNR w KQkq - 0 1").is_empty());
}

#[test]
fn enumeration_does_not_mutate_the_position() {
    let fen = "k7/8/8/8/8/8/PBQK4/n7 b - - 0 1";
    let pos = Position::from_fen(fen).unwrap();
    let _ = desperado_opportunities(&pos);
    assert_eq!(
        pos.to_fen(),
        fen,
        "desperado enumeration must not mutate the board"
    );
}
