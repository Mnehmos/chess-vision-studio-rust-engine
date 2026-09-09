//! TEMPORARY adversarial panic/FP fuzz for the capture-attacker detector.
//! Deleted before merge — process artifact, not a shipped test.
//!
//! 1. Random playouts: the detector must never panic and must be deterministic
//!    (serialize twice, byte-equal), including the opponent counterfactual probe.
//! 2. Every emitted opportunity is adjudicated:
//!    a. structural invariants (legal move, enemy non-king attacker on mv.to,
//!       non-empty sorted protection, positive saved value);
//!    b. an INDEPENDENT full-board material quiescence (re-implemented here, not
//!       the detector's helper) must confirm banked - enemy_extraction >= 0;
//!    c. ENGINE ARBITER: a Searcher probe of the post-move board — the enemy's
//!       search score must not reveal a material collapse the detector missed.
use cvs_bitboard_core::eval::ValueWeights;
use cvs_bitboard_core::facts::motifs::{
    capture_attacker_opportunities, capture_attacker_opportunities_for,
};
use cvs_bitboard_core::facts::{CaptureAttackerOpportunity, FactCollection};
use cvs_bitboard_core::movegen::generate_legal;
use cvs_bitboard_core::search::{SearchOptions, Searcher};
use cvs_bitboard_core::{Piece, Position};

const SEE_VALUE: [i32; 6] = [100, 320, 330, 500, 900, 20000];

struct Rng(u64);
impl Rng {
    fn next(&mut self, n: usize) -> usize {
        self.0 ^= self.0 << 13;
        self.0 ^= self.0 >> 7;
        self.0 ^= self.0 << 17;
        (self.0 % n as u64) as usize
    }
}

/// Independent alternating capture/promotion material minimax with stand-pat.
fn independent_quiesce(pos: &Position, depth: u8) -> i32 {
    if depth == 0 {
        return 0;
    }
    let mut gen = pos.clone();
    let mut best = 0;
    for m in generate_legal(&mut gen) {
        let mut swing = match pos.piece_at(m.to) {
            Some((_, p)) => SEE_VALUE[p.index()],
            None => 0,
        };
        if let Some(promo) = m.flag.promo_piece() {
            swing += SEE_VALUE[promo.index()] - SEE_VALUE[Piece::Pawn.index()];
        }
        if swing == 0 {
            continue;
        }
        let mut after = pos.clone();
        after.make(m);
        let s = swing - independent_quiesce(&after, depth - 1);
        if s > best {
            best = s;
        }
    }
    best
}

fn ops(pos: &Position) -> Vec<CaptureAttackerOpportunity> {
    match capture_attacker_opportunities(pos) {
        FactCollection::Computed { items } => items,
        other => panic!("side-to-move probe must be computed, got {other:?}"),
    }
}

#[test]
fn fuzz_no_panics_deterministic_and_no_false_positives() {
    let mut rng = Rng(0x5eed_cafe_f00d_1234);
    let mut searcher = Searcher::new(ValueWeights::default(), None);
    let mut positions = 0u32;
    let mut emitted = 0u32;
    let mut arbiter_flags: Vec<String> = Vec::new();

    for _game in 0..220 {
        let mut pos = Position::from_fen("rnbqkbnr/pppppppp/8/8/8/8/PPPPPPPP/RNBQKBNR w KQkq - 0 1")
            .unwrap();
        for _ply in 0..90 {
            let mut gen = pos.clone();
            let legal = generate_legal(&mut gen);
            if legal.is_empty() {
                break;
            }

            positions += 1;
            // (1) No panics + determinism, both probes.
            let a = ops(&pos);
            let b = ops(&pos);
            assert_eq!(
                serde_json::to_string(&a).unwrap(),
                serde_json::to_string(&b).unwrap(),
                "nondeterministic output at {}",
                pos.to_fen()
            );
            let _ = capture_attacker_opportunities_for(&pos, pos.stm.flip()); // must not panic

            // (2) Adjudicate every emission.
            for op in &a {
                emitted += 1;
                let mv = legal
                    .iter()
                    .find(|m| m.to_uci() == op.move_uci)
                    .unwrap_or_else(|| {
                        panic!("emitted move {} not legal at {}", op.move_uci, pos.to_fen())
                    });
                assert!(mv.flag.is_capture(), "non-capture emitted at {}", pos.to_fen());
                let (a_color, a_piece) = pos
                    .piece_at(mv.to)
                    .unwrap_or_else(|| panic!("no attacker on {} at {}", op.move_uci, pos.to_fen()));
                assert_eq!(a_color, pos.stm.flip(), "attacker not enemy at {}", pos.to_fen());
                assert!(a_piece != Piece::King, "king 'captured' at {}", pos.to_fen());
                assert!(!op.protected.is_empty(), "empty protection at {}", pos.to_fen());
                assert!(op.material_gain > 0, "non-positive save at {}", pos.to_fen());
                let mut sorted = op.protected.clone();
                sorted.sort_by(|x, y| x.id.cmp(&y.id));
                assert_eq!(sorted, op.protected, "unsorted protection at {}", pos.to_fen());

                // (2b) Independent whole-board debit: the enemy must not extract more
                // than the banked attacker from the post-move board.
                let banked = SEE_VALUE[a_piece.index()];
                let mut after = pos.clone();
                after.make(*mv);
                let extraction = independent_quiesce(&after, 5);
                assert!(
                    banked - extraction >= 0,
                    "FP: banked {banked} < extraction {extraction} for {} at {}",
                    op.move_uci,
                    pos.to_fen()
                );

                // (2c) Engine arbiter: search the post-move board from the enemy side
                // and the pre-move board from our side. Playing a sound defusal can be
                // sub-optimal but must not be a material collapse.
                let opts = SearchOptions {
                    depth: 8,
                    ..Default::default()
                };
                let mut before_probe = pos.clone();
                let r_before = searcher.search(&mut before_probe, opts.clone());
                let mut after_probe = after.clone();
                let r_after = searcher.search(&mut after_probe, opts);
                if r_before.mate.is_none() && r_after.mate.is_none() {
                    let delta = -r_after.score_cp - r_before.score_cp;
                    if delta < -400 {
                        arbiter_flags.push(format!(
                            "delta {delta} for {} at {} (before {}, after-from-enemy {})",
                            op.move_uci,
                            pos.to_fen(),
                            r_before.score_cp,
                            r_after.score_cp
                        ));
                    }
                }
            }

            let m = legal[rng.next(legal.len())];
            pos.make(m);
        }
    }

    println!("fuzzed {positions} positions, {emitted} emissions");
    assert!(
        arbiter_flags.is_empty(),
        "engine arbiter flagged {} suspicious emissions:\n{}",
        arbiter_flags.len(),
        arbiter_flags.join("\n")
    );
    assert!(positions > 10_000, "fuzz did not cover enough positions");
}
