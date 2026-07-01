use cvs_bitboard_core::facts::motifs::win_exchange_opportunities;
use cvs_bitboard_core::facts::{FactCollection, PieceType, WinExchangeOpportunity};
use cvs_bitboard_core::Position;

fn we(fen: &str) -> Vec<WinExchangeOpportunity> {
    let pos = Position::from_fen(fen).unwrap();
    match win_exchange_opportunities(&pos) {
        FactCollection::Computed { items } => items,
        other => panic!("win-the-exchange should be computed, got {other:?}"),
    }
}

// ── Positives ────────────────────────────────────────────────────────────────

#[test]
fn validates_a_bishop_winning_the_exchange() {
    // White to move: bishop a1 captures the black rook on g7 (a1-h8 diagonal, all clear),
    // which is defended by the black pawn on h8. The full SEE swap is 500 (rook) − 330
    // (bishop recaptured by hxg7) = 170, squarely in the rook-for-minor band. R-for-B.
    let items = we("4k2p/6r1/8/8/8/8/8/B3K3 w - - 0 1");
    let x = items
        .iter()
        .find(|x| x.move_uci == "a1g7")
        .expect("Bxg7 winning a rook for a bishop should be validated");
    assert_eq!(x.kind, "win_the_exchange");
    assert_eq!(x.validator, "win_exchange_validation");
    assert_eq!(x.mover.piece_type, PieceType::Bishop);
    assert_eq!(x.mover.square, "g7");
    assert_eq!(x.victim.piece_type, PieceType::Rook);
    assert_eq!(x.victim.square, "g7");
    assert!(!x.gives_check);
    assert_eq!(x.material_gain, 170);
}

#[test]
fn validates_a_knight_winning_the_exchange() {
    // White to move: knight c2 captures the black rook on d4 (a knight leap), defended by
    // the black pawn on c5. SEE = 500 − 320 = 180, in band. R-for-N.
    let items = we("4k3/8/8/2p5/3r4/8/2N5/4K3 w - - 0 1");
    let x = items
        .iter()
        .find(|x| x.move_uci == "c2d4")
        .expect("Nxd4 winning a rook for a knight should be validated");
    assert_eq!(x.mover.piece_type, PieceType::Knight);
    assert_eq!(x.mover.square, "d4");
    assert_eq!(x.victim.piece_type, PieceType::Rook);
    assert_eq!(x.victim.square, "d4");
    assert!(!x.gives_check);
    assert_eq!(x.material_gain, 180);
}

#[test]
fn validates_a_through_recapture_win_in_band() {
    // White to move: bishop b2 captures the black rook on e5, which is defended by BOTH the
    // pawn on f6 and the rook on e8 (backing on the e-file); our own rook on e1 backs the
    // e5 square. The full multi-capture swap — Bxe5 fxe5 Rxe5 Rxe5 minimaxed — still resolves
    // to +170, proving the detector classifies a deep exchange, not just a single-square one.
    let items = we("k3r3/8/5p2/4r3/8/8/1B6/4R1K1 w - - 0 1");
    let x = items
        .iter()
        .find(|x| x.move_uci == "b2e5")
        .expect("Bxe5 through-recapture winning the exchange should be validated");
    assert_eq!(x.mover.piece_type, PieceType::Bishop);
    assert_eq!(x.victim.piece_type, PieceType::Rook);
    assert_eq!(x.victim.square, "e5");
    assert_eq!(x.material_gain, 170);
}

// ── Negatives ────────────────────────────────────────────────────────────────

#[test]
fn does_not_report_a_hanging_rook_grab() {
    // Bishop a1 captures an UNDEFENDED rook on g7: SEE = 500, ABOVE the exchange band. This
    // is a plain hanging-piece win (counting / piece_safety), not the exchange.
    let items = we("4k3/6r1/8/8/8/8/8/B3K3 w - - 0 1");
    assert!(
        items.iter().all(|x| x.move_uci != "a1g7"),
        "a plain hanging-rook grab (~500) is out of the exchange band, got {items:?}"
    );
}

#[test]
fn does_not_report_an_equal_minor_trade() {
    // Bishop a1 captures a black knight on g7 defended by the pawn on h8: SEE ≈ 0 (−10),
    // BELOW the band and not a rook victim.
    let items = we("4k2p/6n1/8/8/8/8/8/B3K3 w - - 0 1");
    assert!(
        items.iter().all(|x| x.move_uci != "a1g7"),
        "an equal/near minor trade is not winning the exchange, got {items:?}"
    );
}

#[test]
fn does_not_report_a_rook_capturing_a_rook() {
    // Rook d1 captures the black rook on d5 (defended by the c6 pawn): the attacker is NOT a
    // minor, so the attacker-profile guard rejects it even though the net is ~0.
    let items = we("4k3/8/2p5/3r4/8/8/8/3RK3 w - - 0 1");
    assert!(
        items.iter().all(|x| x.move_uci != "d1d5"),
        "a rook capturing a rook cannot win the exchange, got {items:?}"
    );
}

#[test]
fn does_not_report_capturing_a_queen() {
    // Bishop b2 captures a black QUEEN on e5: that is winning the queen (SEE ≈ 570), not the
    // exchange. The victim-profile guard requires a ROOK victim.
    let items = we("4k3/8/3p4/4q3/8/8/1B6/4K3 w - - 0 1");
    assert!(
        items.iter().all(|x| x.move_uci != "b2e5"),
        "capturing a queen is not winning the exchange, got {items:?}"
    );
}

#[test]
fn rejects_a_pinned_minor() {
    // Bishop c3 sits on the a1-h8 diagonal aiming at the black rook on g7, but it is
    // absolutely pinned to the white king on c1 by the black rook on c8 (c-file). see()
    // ignores the pin and would score Bxg7 at +170, but the bishop can NEVER legally leave
    // the c-file: generate_legal drops Bxg7, so it is never a candidate. Load-bearing
    // regression for the pinned-attacker false-positive class.
    let items = we("2r1k3/6r1/8/8/8/2B5/8/2K5 w - - 0 1");
    assert!(
        items.iter().all(|x| x.move_uci != "c3g7"),
        "a pinned minor cannot execute the capture, got {items:?}"
    );
}

#[test]
fn rejects_a_win_refuted_by_a_counter_capture() {
    // Knight c3 captures the black rook on d5 (defended by the c6 pawn): raw SEE = 180, in
    // band. BUT Nxd5 vacates c3, opening the a1-h8 diagonal, and the black bishop on h8
    // answers Bxa1 winning our hanging queen. The legality-aware worst-case debits the queen
    // (500 rook banked − 900 queen lost < 0), so the "win" is refuted and dropped.
    let items = we("4k2b/8/2p5/3r4/8/2N5/8/Q3K3 w - - 0 1");
    assert!(
        items.iter().all(|x| x.move_uci != "c3d5"),
        "an exchange refuted by a bigger counter-capture is not a sound win, got {items:?}"
    );
}

#[test]
fn no_win_exchange_in_the_opening_position() {
    assert!(
        we("rnbqkbnr/pppppppp/8/8/8/8/PPPPPPPP/RNBQKBNR w KQkq - 0 1").is_empty(),
        "the start position has no win-the-exchange captures"
    );
}

#[test]
fn enumeration_does_not_mutate_the_position() {
    let fen = "4k2p/6r1/8/8/8/8/8/B3K3 w - - 0 1";
    let pos = Position::from_fen(fen).unwrap();
    let _ = win_exchange_opportunities(&pos);
    assert_eq!(
        pos.to_fen(),
        fen,
        "win-the-exchange enumeration must not mutate the board"
    );
}

#[test]
fn emitted_opportunities_report_a_positive_in_band_gain() {
    // Every reported win-the-exchange carries a proven positive gain inside the
    // rook-for-minor band (~150..185).
    for fen in [
        "4k2p/6r1/8/8/8/8/8/B3K3 w - - 0 1",
        "4k3/8/8/2p5/3r4/8/2N5/4K3 w - - 0 1",
        "k3r3/8/5p2/4r3/8/8/1B6/4R1K1 w - - 0 1",
    ] {
        for x in we(fen) {
            assert!(
                (150..=185).contains(&x.material_gain),
                "win-the-exchange on {fen} reported out-of-band gain: {x:?}"
            );
            assert_eq!(x.mover.piece_type == PieceType::Bishop
                || x.mover.piece_type == PieceType::Knight, true,
                "mover must be a minor: {x:?}");
            assert_eq!(x.victim.piece_type, PieceType::Rook, "victim must be a rook: {x:?}");
        }
    }
}
