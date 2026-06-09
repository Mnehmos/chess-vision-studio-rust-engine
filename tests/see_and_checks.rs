//! R1 tactical-primitive tests: SEE (parity with the legacy TS `see` cases) plus
//! check detection (in_check / gives_check / attackers_to_color).
use cvs_bitboard_core::attacks::attackers_to_color;
use cvs_bitboard_core::movegen::{generate_legal, gives_check, in_check};
use cvs_bitboard_core::see::see;
use cvs_bitboard_core::{Color, MoveFlag, Position};

/// Algebraic square ("e4") → index (LERF).
fn sq(s: &str) -> u8 {
    let b = s.as_bytes();
    (b[1] - b'1') * 8 + (b[0] - b'a')
}

fn see_at(fen: &str, from: &str, to: &str) -> i32 {
    let pos = Position::from_fen(fen).unwrap();
    see(&pos, sq(from), sq(to))
}

// --- SEE (same cases as the legacy TS see.test.ts) ---

#[test]
fn see_wins_free_pawn() {
    // exd5 wins an undefended pawn.
    assert_eq!(see_at("4k3/8/8/3p4/4P3/8/8/4K3 w - - 0 1", "e4", "d5"), 100);
}

#[test]
fn see_equal_trade_is_zero() {
    // d5 is defended by the c6 pawn: pawn for pawn.
    assert_eq!(see_at("4k3/8/2p5/3p4/4P3/8/8/4K3 w - - 0 1", "e4", "d5"), 0);
}

#[test]
fn see_queen_grabs_defended_pawn_is_negative() {
    // Queen wins a pawn (100) then is taken by the c6 pawn (loses 900).
    assert_eq!(see_at("4k3/8/2p5/3p4/8/8/3Q4/4K3 w - - 0 1", "d2", "d5"), 100 - 900);
}

#[test]
fn see_quiet_move_to_safe_square_is_zero() {
    assert_eq!(see_at("4k3/8/8/8/8/8/3N4/4K3 w - - 0 1", "d2", "f3"), 0);
}

#[test]
fn see_into_undefended_attacked_square_is_negative() {
    // Knight steps onto d5, attacked by the e6 pawn, undefended.
    assert_eq!(see_at("4k3/8/4p3/8/8/4N3/8/4K3 w - - 0 1", "e3", "d5"), -320);
}

#[test]
fn see_xray_rook_battery() {
    // White rooks doubled on the e-file vs a single black rook on e8; e6 is a black
    // pawn defended only by the rook. Rxe6 wins the pawn through the x-ray battery.
    // White: Re1,Re2 ; Black: pe6, Re8.  Rxe6: +pawn(100), Rxe6, RxR, RxR → +100.
    let v = see_at("4r1k1/8/4p3/8/8/8/4R3/4R1K1 w - - 0 1", "e2", "e6");
    assert_eq!(v, 100);
}

// --- check primitives ---

#[test]
fn in_check_detects_bishop_check() {
    // Black bishop h4 rakes e1 (g3,f2 empty) — White king is in check.
    let pos = Position::from_fen("4k3/8/8/8/7b/8/8/4K3 w - - 0 1").unwrap();
    assert!(in_check(&pos));
}

#[test]
fn in_check_false_at_startpos() {
    assert!(!in_check(&Position::startpos()));
}

#[test]
fn gives_check_rook_to_back_rank() {
    // Ra1-a8 delivers check along the 8th rank to the black king on e8.
    let mut pos = Position::from_fen("4k3/8/8/8/8/8/8/R3K3 w - - 0 1").unwrap();
    let legal = generate_legal(&mut pos);
    let ra8 = legal.iter().find(|m| m.from == sq("a1") && m.to == sq("a8")).copied();
    assert!(ra8.is_some(), "Ra8 should be legal");
    assert!(gives_check(&mut pos, ra8.unwrap()));
    // A quiet king step does not give check.
    let kf2 = legal.iter().find(|m| m.from == sq("e1") && m.flag == MoveFlag::Quiet).copied();
    if let Some(m) = kf2 {
        assert!(!gives_check(&mut pos, m));
    }
}

#[test]
fn attackers_to_color_counts_startpos_f3() {
    // f3 is attacked by the g1 knight, the e2 pawn, and the g2 pawn (3 white attackers).
    let pos = Position::startpos();
    let atk = attackers_to_color(&pos, sq("f3"), Color::White, pos.all);
    assert_eq!(atk.count_ones(), 3);
}
