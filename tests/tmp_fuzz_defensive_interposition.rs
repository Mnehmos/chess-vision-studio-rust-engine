//! TEMP adversarial fuzz for the defensive-interposition detector — DELETED BEFORE COMMIT.
//!
//! 1. Panic/determinism fuzz over random playouts from varied seeds.
//! 2. Structural invariants on every emitted op.
//! 3. Independent full-width material negamax oracle (4 plies + capture tail, EP-correct):
//!    after the claimed save, the enemy must not have a forced material win. Flags are
//!    printed for manual/arbiter review rather than hard-failed (quiet unrelated tactics
//!    are out of the static claim's scope), but we hard-fail on blatant refutations.

use cvs_bitboard_core::facts::motifs::{
    defensive_interposition_opportunities, defensive_interposition_opportunities_for,
};
use cvs_bitboard_core::facts::{DefensiveInterpositionOpportunity, FactCollection};
use cvs_bitboard_core::movegen::generate_legal;
use cvs_bitboard_core::see::SEE_VALUE;
use cvs_bitboard_core::{Color, Piece, Position};

struct Rng(u64);
impl Rng {
    fn next(&mut self) -> u64 {
        let mut x = self.0;
        x ^= x << 13;
        x ^= x >> 7;
        x ^= x << 17;
        self.0 = x;
        x
    }
    fn pick(&mut self, n: usize) -> usize {
        (self.next() % n as u64) as usize
    }
}

/// stm-relative absolute material on the SEE scale (kings excluded).
fn mat(pos: &Position) -> i32 {
    let mut w = 0;
    let mut b = 0;
    for p in [
        Piece::Pawn,
        Piece::Knight,
        Piece::Bishop,
        Piece::Rook,
        Piece::Queen,
    ] {
        w += pos.pieces[Color::White.index()][p.index()].count_ones() as i32 * SEE_VALUE[p.index()];
        b += pos.pieces[Color::Black.index()][p.index()].count_ones() as i32 * SEE_VALUE[p.index()];
    }
    if pos.stm == Color::White {
        w - b
    } else {
        b - w
    }
}

/// Capture/promotion quiescence on absolute stm-relative material, with stand pat.
fn qmat(pos: &Position, depth: u8) -> i32 {
    let stand = mat(pos);
    if depth == 0 {
        return stand;
    }
    let mut gen = pos.clone();
    let mut best = stand;
    for m in generate_legal(&mut gen) {
        if !m.flag.is_capture() && m.flag.promo_piece().is_none() {
            continue;
        }
        let mut after = pos.clone();
        after.make(m);
        let v = -qmat(&after, depth - 1);
        if v > best {
            best = v;
        }
    }
    best
}

/// Full-width material negamax to `depth` plies, capture tail at the leaves.
fn negamat(pos: &Position, depth: u8, qdepth: u8) -> i32 {
    if depth == 0 {
        return qmat(pos, qdepth);
    }
    let mut gen = pos.clone();
    let legal = generate_legal(&mut gen);
    if legal.is_empty() {
        return mat(pos); // mate/stalemate: a material oracle just reports material
    }
    let mut best = i32::MIN;
    for m in legal {
        let mut after = pos.clone();
        after.make(m);
        let v = -negamat(&after, depth - 1, qdepth);
        if v > best {
            best = v;
        }
    }
    best
}

fn items(pos: &Position) -> Vec<DefensiveInterpositionOpportunity> {
    match defensive_interposition_opportunities(pos) {
        FactCollection::Computed { items } => items,
        other => panic!("should be computed, got {other:?}"),
    }
}

fn sq_index(name: &str) -> u8 {
    let b = name.as_bytes();
    (b[0] - b'a') + 8 * (b[1] - b'1')
}

const SEEDS: [&str; 12] = [
    "rnbqkbnr/pppppppp/8/8/8/8/PPPPPPPP/RNBQKBNR w KQkq - 0 1",
    "r1bqkb1r/pppp1ppp/2n2n2/4p3/2B1P3/5N2/PPPP1PPP/RNBQK2R w KQkq - 4 4",
    "r1bq1rk1/pp2ppbp/2np1np1/8/3NP3/2N1BP2/PPPQ2PP/R3KB1R w KQ - 0 9",
    "r2q1rk1/1b2bppp/p2ppn2/1p6/3NPP2/2N1B3/PPP3PP/R2QR1K1 w - - 0 12",
    "2rq1rk1/pb1nbppp/1p2pn2/2p5/2BP4/1PN1PN2/PB3PPP/R2Q1RK1 w - - 0 11",
    "r3kb1r/1bqn1ppp/p3p3/1p1pP3/3N4/2N1B3/PPP1QPPP/2KR3R w kq - 2 12",
    "5rk1/1p3ppp/pq1p4/2nP4/2P1r3/1P2B1P1/P3QP1P/R4RK1 w - - 1 22",
    "8/2p2k2/1p1p1p2/p2P1P2/P1P3p1/1P4P1/5K2/8 w - - 0 40",
    "2r3k1/5ppp/p2p4/1p1Pn3/1P2P3/P4B1P/5PP1/3R2K1 b - - 2 28",
    "r4rk1/pp1n1ppp/2pb1q2/3p4/3P1B2/2NBP3/PPQ2PPP/R4RK1 b - - 6 12",
    "6k1/pp3pp1/2p1b2p/8/2P1B3/1P4P1/P4P1P/6K1 b - - 0 30",
    "1k1r3r/ppq2ppp/2pbpn2/8/3P4/2N1PN2/PPQ2PPP/2KR1B1R b - - 4 13",
];

#[test]
fn tmp_fuzz_no_panics_and_no_false_positives() {
    let mut rng = Rng(0x9E3779B97F4A7C15);
    let mut positions_checked = 0u32;
    let mut ops_emitted = 0u32;
    let mut flags = 0u32;

    for (si, seed) in SEEDS.iter().enumerate() {
        for playout in 0..6 {
            let mut pos = Position::from_fen(seed).unwrap();
            for ply in 0..44 {
                // ── run the detector (panic + determinism + probe parity) ──
                let a = items(&pos);
                let b = items(&pos);
                assert_eq!(a, b, "nondeterministic output at {}", pos.to_fen());
                let via_for = match defensive_interposition_opportunities_for(&pos, pos.stm) {
                    FactCollection::Computed { items } => items,
                    other => panic!("stm probe should be computed, got {other:?}"),
                };
                assert_eq!(a, via_for, "stm-probe mismatch at {}", pos.to_fen());
                // opponent probe must never panic (may be Unavailable in check)
                let _ = defensive_interposition_opportunities_for(&pos, pos.stm.flip());
                positions_checked += 1;

                // ── verify every emitted op ──
                let mut gen = pos.clone();
                let legal = generate_legal(&mut gen);
                for op in &a {
                    ops_emitted += 1;
                    let mv = legal
                        .iter()
                        .copied()
                        .find(|m| m.to_uci() == op.move_uci)
                        .unwrap_or_else(|| {
                            panic!("op move {} not legal at {}", op.move_uci, pos.to_fen())
                        });
                    // structural invariants
                    let g_sq = sq_index(&op.protected.square);
                    let a_sq = sq_index(&op.cut_attacker.square);
                    let s_sq = sq_index(&op.interposer.square);
                    assert_eq!(s_sq, mv.to, "interposer square is the landing square");
                    assert!(op.material_gain > 0, "gain must be positive");
                    assert_eq!(op.kind, "defensive_interposition");
                    assert_eq!(op.validator, "defensive_interposition_validation");
                    assert!(op.ray.iter().any(|r| sq_index(r) == s_sq), "S on the ray");
                    let (gc, gp) = pos.piece_at(g_sq).expect("protected piece exists");
                    assert_eq!(gc, pos.stm, "protected is ours");
                    assert!(gp != Piece::King, "protected is never the king");
                    let (ac, ap) = pos.piece_at(a_sq).expect("attacker exists");
                    assert_eq!(ac, pos.stm.flip(), "attacker is the enemy's");
                    assert!(
                        matches!(ap, Piece::Bishop | Piece::Rook | Piece::Queen),
                        "attacker is a slider"
                    );

                    // ── independent oracle: after the save, the enemy (to move) must not
                    //    have a forced material win within 4 plies + capture tail ──
                    let mut after = pos.clone();
                    after.make(mv);
                    let baseline = mat(&after);
                    let forced = negamat(&after, 4, 8);
                    if forced > baseline {
                        flags += 1;
                        println!(
                            "FLAG (+{} for enemy within 4 plies): fen={} op={:?}",
                            forced - baseline,
                            pos.to_fen(),
                            op
                        );
                        // hard-fail on blatant refutation: enemy nets >= a clean minor
                        assert!(
                            forced - baseline < 300,
                            "blatant refutation of a claimed save at {}",
                            pos.to_fen()
                        );
                    }
                }

                // ── advance the playout (bias to captures a third of the time) ──
                if legal.is_empty() {
                    break;
                }
                let caps: Vec<_> = legal.iter().copied().filter(|m| m.flag.is_capture()).collect();
                let m = if !caps.is_empty() && rng.pick(3) == 0 {
                    caps[rng.pick(caps.len())]
                } else {
                    legal[rng.pick(legal.len())]
                };
                pos.make(m);
                let _ = (si, playout, ply);
            }
        }
    }
    println!(
        "fuzz done: {positions_checked} positions, {ops_emitted} ops emitted, {flags} flags"
    );
}
