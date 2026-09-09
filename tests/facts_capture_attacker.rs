use cvs_bitboard_core::facts::motifs::capture_attacker_opportunities;
use cvs_bitboard_core::facts::{CaptureAttackerOpportunity, FactCollection, PieceType};
use cvs_bitboard_core::Position;

fn ca(fen: &str) -> Vec<CaptureAttackerOpportunity> {
    let pos = Position::from_fen(fen).unwrap();
    match capture_attacker_opportunities(&pos) {
        FactCollection::Computed { items } => items,
        other => panic!("capture-the-attacker should be computed, got {other:?}"),
    }
}

// ── Positives ────────────────────────────────────────────────────────────────

#[test]
fn validates_capturing_the_attacker_of_a_queen() {
    // White to move: black Ne4 legally wins Qd2 (900, undefended — Kg1 does not touch
    // d2). Nc3xe4 removes the attacker; Ne4 also eyes c3 == mv.from, exercising the
    // capturer-square exclusion. The knight is banked (320) and black has no reply
    // capture, so the whole-board debit passes.
    let items = ca("6k1/8/8/8/4n3/2N5/3Q4/6K1 w - - 0 1");
    let x = items
        .iter()
        .find(|x| x.move_uci == "c3e4")
        .expect("Nxe4 defusing the threat on the queen should be validated");
    assert_eq!(x.kind, "capture_the_attacker");
    assert_eq!(x.validator, "capture_attacker_validation");
    assert_eq!(x.mover.piece_type, PieceType::Knight);
    assert_eq!(x.mover.square, "e4");
    assert_eq!(x.captured_attacker.piece_type, PieceType::Knight);
    assert_eq!(x.captured_attacker.square, "e4");
    assert_eq!(x.protected.len(), 1);
    assert_eq!(x.protected[0].id, "white-queen-d2");
    assert!(!x.gives_check);
    assert_eq!(x.material_gain, 900);
}

#[test]
fn validates_a_defusing_capture_with_tempo() {
    // Black Rd5 legally wins Bd1 down the d-file. Rh5xd5 removes it AND checks Kd7
    // (the d-file), so black's replies are evasions only — the capture-with-tempo
    // variant that remove_guard structurally cannot see. No recapture exists:
    // banked 500 − quiescence 0.
    let items = ca("8/3k4/8/3r3R/8/8/8/3B2K1 w - - 0 1");
    let x = items
        .iter()
        .find(|x| x.move_uci == "h5d5")
        .expect("Rxd5+ defusing the threat on the bishop should be validated");
    assert_eq!(x.mover.piece_type, PieceType::Rook);
    assert_eq!(x.mover.square, "d5");
    assert_eq!(x.captured_attacker.piece_type, PieceType::Rook);
    assert_eq!(x.protected.len(), 1);
    assert_eq!(x.protected[0].id, "white-bishop-d1");
    assert!(x.gives_check, "Rxd5 checks the king on d7");
    assert_eq!(x.material_gain, 330);
}

#[test]
fn validates_an_even_trade_that_defuses() {
    // Black Re8 legally wins Be3 down the e-file (and eyes Ra8 == mv.from, excluded).
    // Ra8xe8 defuses; Kf7 recaptures on e8, so banked 500 − quiescence 500 == 0 —
    // the exact >= 0 boundary: an even RxR trade that saves the bishop IS the motif.
    let items = ca("R3r3/5k2/8/8/8/4B3/8/6K1 w - - 0 1");
    let x = items
        .iter()
        .find(|x| x.move_uci == "a8e8")
        .expect("RxR even-trade defusal should be validated at the >= 0 boundary");
    assert_eq!(x.mover.piece_type, PieceType::Rook);
    assert_eq!(x.mover.square, "e8");
    assert_eq!(x.captured_attacker.piece_type, PieceType::Rook);
    assert_eq!(x.protected.len(), 1);
    assert_eq!(x.protected[0].id, "white-bishop-e3");
    assert!(!x.gives_check);
    assert_eq!(x.material_gain, 330);
}

#[test]
fn validates_a_bishop_removing_a_rook_attacker() {
    // Black Re4 legally wins Ne2 (Kg1 does not defend e2). Bc2xe4 removes the rook;
    // the rook is undefended, so the debit banks 500 against nothing. The black king
    // sits on h8 deliberately — on h7 the bishop itself would pin Re4 and the threat
    // would be fake (see the pinned-attacker negative below).
    let items = ca("7k/8/8/8/4r3/8/2B1N3/6K1 w - - 0 1");
    let x = items
        .iter()
        .find(|x| x.move_uci == "c2e4")
        .expect("Bxe4 defusing the threat on the knight should be validated");
    assert_eq!(x.mover.piece_type, PieceType::Bishop);
    assert_eq!(x.mover.square, "e4");
    assert_eq!(x.captured_attacker.piece_type, PieceType::Rook);
    assert_eq!(x.protected.len(), 1);
    assert_eq!(x.protected[0].id, "white-knight-e2");
    assert!(!x.gives_check);
    assert_eq!(x.material_gain, 320);
}

#[test]
fn validates_the_color_mirror() {
    // The queen-defusal positive mirrored vertically with colors swapped, black to
    // move: white Ne5 threatens Qd7 (and c6 == mv.from); Nc6xe5 defuses.
    let items = ca("6k1/3q4/2n5/4N3/8/8/8/6K1 b - - 0 1");
    let x = items
        .iter()
        .find(|x| x.move_uci == "c6e5")
        .expect("the mirrored Nxe5 defusal should be validated for black");
    assert_eq!(x.mover.piece_type, PieceType::Knight);
    assert_eq!(x.protected.len(), 1);
    assert_eq!(x.protected[0].id, "black-queen-d7");
    assert!(!x.gives_check);
    assert_eq!(x.material_gain, 900);
}

#[test]
fn counterfactual_probe_reports_the_non_moving_side() {
    // Same board with BLACK to move: the white-side probe is a legal counterfactual
    // (black is not in check), and it must surface white's c3e4 defusal.
    use cvs_bitboard_core::facts::motifs::capture_attacker_opportunities_for;
    use cvs_bitboard_core::Color;
    let pos = Position::from_fen("6k1/8/8/8/4n3/2N5/3Q4/6K1 b - - 0 1").unwrap();
    let items = match capture_attacker_opportunities_for(&pos, Color::White) {
        FactCollection::Computed { items } => items,
        other => panic!("white counterfactual probe should be computed, got {other:?}"),
    };
    assert!(
        items.iter().any(|x| x.move_uci == "c3e4"),
        "the counterfactual probe must see white's defusal, got {items:?}"
    );
}

// ── Negatives ────────────────────────────────────────────────────────────────

#[test]
fn rejects_when_a_second_attacker_remains() {
    // Bd5 is lost to BOTH Nc3 and Rd8. Capturing the knight (b2c3) does not defuse:
    // with Nc3 deleted and the pawn still home, Rd8xd5 still wins the bishop, so the
    // causality gate (removal alone must kill the threat) rejects. Refutation: 1.bxc3 Rxd5.
    let items = ca("3r3k/8/8/3B4/8/2n5/1P6/6K1 w - - 0 1");
    assert!(
        items.is_empty(),
        "capturing one of two attackers defuses nothing, got {items:?}"
    );
}

#[test]
fn rejects_a_capture_that_hangs_the_capturer() {
    // Black Ne4 legally wins Rd2 (net 180 after the queen's rank-2 recapture — the
    // trigger and causality gates PASS). But Qg2xe4 hangs the queen to f5xe4: banked
    // 320 − quiescence 900 < 0, so the whole-board debit rejects. Refutation: 1.Qxe4 fxe4.
    let items = ca("1k6/8/8/5p2/4n3/8/3R2Q1/6K1 w - - 0 1");
    assert!(
        items.is_empty(),
        "a defusal that loses the queen for a knight is unsound, got {items:?}"
    );
}

#[test]
fn rejects_when_collateral_outweighs_the_defusal() {
    // The queen-attacker position PLUS one poisoned white rook on a7. Nc3xe4 still
    // "saves" Qd2 on every single-square probe, but black's best reply ignores the
    // trade: Bb8xa7 grabs the hanging rook (banked 320 − quiescence 500 < 0). Only
    // the FULL-BOARD quiescence sees the off-square collateral. Refutation: 1.Nxe4 Bxa7.
    let items = ca("1b4k1/R7/8/8/4n3/2N5/3Q4/6K1 w - - 0 1");
    assert!(
        items.is_empty(),
        "a defusal refuted by off-square collateral is unsound, got {items:?}"
    );
}

#[test]
fn rejects_a_pinned_fake_attacker() {
    // Rd4 "attacks" Nf4 geometrically but is absolutely pinned by Rd1 to Kd8, so
    // generate_legal drops Rxf4 and the legal-loss trigger finds nothing to save:
    // there was never a threat to defuse. Both captures of the rook (c2d4, d1d4)
    // must stay silent. (Rd1 as a rescue target also dies: Rxd1 Kxd1 is SEE 0.)
    let items = ca("3k4/8/8/8/3r1N2/8/2N5/3RK3 w - - 0 1");
    assert!(
        items.is_empty(),
        "a pinned attacker is a fake threat — nothing to defuse, got {items:?}"
    );
}

#[test]
fn rejects_when_the_attacker_is_pinned_to_our_capturer_line() {
    // The bishop-removes-rook positive with the black king moved h8 -> h7: now Bc2
    // itself pins Re4 on the c2-h7 diagonal, Rxe2 is illegal, and the "threat" on the
    // knight is fake — the trigger dies and Bxe4 emits nothing (a plain winning
    // capture belongs to piece_safety/counting, not this motif).
    let items = ca("8/7k/8/8/4r3/8/2B1N3/6K1 w - - 0 1");
    assert!(
        items.is_empty(),
        "an attacker pinned by the would-be capturer never threatened, got {items:?}"
    );
}

#[test]
fn rejects_when_the_target_was_never_lost() {
    // Nd2 is attacked by Ne4 but guarded by Qd1: 1...Nxd2 2.Qxd2 was always even
    // (SEE 0), so there is nothing to save — d3e4 is a plain winning capture for
    // counting/piece_safety, not this motif. d2e4 additionally exercises the
    // g_sq != mv.from self-rescue exclusion (escape, not protection).
    let items = ca("6k1/8/8/8/4n3/3P4/3N4/3Q2K1 w - - 0 1");
    assert!(
        items.is_empty(),
        "a defended piece was never lost — nothing to defuse, got {items:?}"
    );
}

#[test]
fn no_capture_attacker_in_the_opening_position() {
    assert!(
        ca("rnbqkbnr/pppppppp/8/8/8/8/PPPPPPPP/RNBQKBNR w KQkq - 0 1").is_empty(),
        "the start position has no capture-the-attacker moves"
    );
}

// ── House invariants ─────────────────────────────────────────────────────────

#[test]
fn enumeration_does_not_mutate_the_position() {
    let fen = "6k1/8/8/8/4n3/2N5/3Q4/6K1 w - - 0 1";
    let pos = Position::from_fen(fen).unwrap();
    let _ = capture_attacker_opportunities(&pos);
    assert_eq!(
        pos.to_fen(),
        fen,
        "capture-the-attacker enumeration must not mutate the board"
    );
}

#[test]
fn enumeration_is_deterministic_across_repeated_runs() {
    for fen in [
        "6k1/8/8/8/4n3/2N5/3Q4/6K1 w - - 0 1",
        "8/3k4/8/3r3R/8/8/8/3B2K1 w - - 0 1",
        "R3r3/5k2/8/8/8/4B3/8/6K1 w - - 0 1",
        "7k/8/8/8/4r3/8/2B1N3/6K1 w - - 0 1",
        "3r3k/8/8/3B4/8/2n5/1P6/6K1 w - - 0 1",
        "1k6/8/8/5p2/4n3/8/3R2Q1/6K1 w - - 0 1",
        "1b4k1/R7/8/8/4n3/2N5/3Q4/6K1 w - - 0 1",
        "3k4/8/8/8/3r1N2/8/2N5/3RK3 w - - 0 1",
        "8/7k/8/8/4r3/8/2B1N3/6K1 w - - 0 1",
        "6k1/8/8/8/4n3/3P4/3N4/3Q2K1 w - - 0 1",
    ] {
        let a = serde_json::to_string(&ca(fen)).unwrap();
        let b = serde_json::to_string(&ca(fen)).unwrap();
        assert_eq!(a, b, "detector output must be deterministic for {fen}");
    }
}

#[test]
fn emitted_opportunities_report_saved_material_and_sorted_protection() {
    // Every reported defusal carries a positive saved value and id-sorted protection.
    for fen in [
        "6k1/8/8/8/4n3/2N5/3Q4/6K1 w - - 0 1",
        "8/3k4/8/3r3R/8/8/8/3B2K1 w - - 0 1",
        "R3r3/5k2/8/8/8/4B3/8/6K1 w - - 0 1",
        "7k/8/8/8/4r3/8/2B1N3/6K1 w - - 0 1",
    ] {
        for x in ca(fen) {
            assert!(x.material_gain > 0, "saved value must be positive: {x:?}");
            assert!(!x.protected.is_empty(), "must protect something: {x:?}");
            let mut sorted = x.protected.clone();
            sorted.sort_by(|a, b| a.id.cmp(&b.id));
            assert_eq!(sorted, x.protected, "protected must be id-sorted: {x:?}");
            assert!(
                x.protected.iter().all(|p| p.piece_type != PieceType::King),
                "a king is never a protected material target: {x:?}"
            );
        }
    }
}
