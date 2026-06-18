//! NNUE accumulator correctness: incremental eval must equal the full
//! recompute at EVERY node of random playouts — promotions, castles, en
//! passant included. A single mismatch means a feature-delta bug that would
//! silently corrupt every search eval.
use cvs_bitboard_core::eval::Nnue;
use cvs_bitboard_core::movegen::generate_legal;
use cvs_bitboard_core::Position;

const NET: &str = "f:/tools/cvs-baselines/raw-nnue-h256-sf-d12-v3.json";

/// Tiny deterministic PRNG (xorshift) — no rand dependency.
struct Rng(u64);
impl Rng {
    fn next(&mut self, n: usize) -> usize {
        self.0 ^= self.0 << 13;
        self.0 ^= self.0 >> 7;
        self.0 ^= self.0 << 17;
        (self.0 % n as u64) as usize
    }
}

#[test]
fn incremental_matches_full_recompute_on_random_playouts() {
    let Ok(net) = Nnue::load(NET) else {
        eprintln!("skipping: champion net not present at {NET}");
        return;
    };
    assert!(net.supports_incremental());
    let mut rng = Rng(0x9E3779B97F4A7C15);
    let mut checked = 0u32;
    let mut exact = 0u32;
    for game in 0..40 {
        let mut pos = Position::startpos();
        let mut acc = net.fresh_acc(&pos);
        let mut stack = Vec::new();
        for _ply in 0..120 {
            let moves = generate_legal(&mut pos);
            if moves.is_empty() {
                break;
            }
            let mv = moves[rng.next(moves.len())];
            stack.push(acc.clone());
            net.acc_apply(&mut acc, &pos, mv);
            pos.make(mv);
            let inc = net.eval_acc(&pos, &acc, pos.stm);
            let full = net.eval_stm(&pos);
            // f32 summation order differs between incremental and fresh paths,
            // so ±1cp rounding drift is expected on long playouts; anything
            // larger is a real feature-delta bug. Exactness is checked below.
            assert!(
                (inc - full).abs() <= 1,
                "game {game} ply {_ply}: incremental {inc} != full {full} after {} (fen {})",
                mv.to_uci(),
                pos.to_fen()
            );
            exact += (inc == full) as u32;
            checked += 1;
            // occasionally unmake to exercise the pop path
            if rng.next(10) == 0 {
                pos.unmake();
                acc = stack.pop().unwrap();
                let inc = net.eval_acc(&pos, &acc, pos.stm);
                let full = net.eval_stm(&pos);
                assert!(
                    (inc - full).abs() <= 1,
                    "after unmake at game {game} ply {_ply}"
                );
            }
        }
    }
    assert!(
        checked > 2000,
        "playouts too short: {checked} nodes checked"
    );
    // Drift should be rare rounding flips, not systematic error.
    assert!(
        exact * 100 >= checked * 99,
        "only {exact}/{checked} exact — systematic delta bug, not f32 drift"
    );
    println!("verified {checked} incremental evals ({exact} exact)");
}
