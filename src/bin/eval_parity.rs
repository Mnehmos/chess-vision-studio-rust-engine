//! Eval parity + speed report: Rust eval vs the legacy TS reference.
//!
//!   cargo run --release --bin eval_parity -- <fixtures.json>
//!
//! The fixture file (exported by the TS `export:eval-fixtures` script) carries the
//! trained mixed weights and, per FEN, the TS `evaluateWhiteFloat` under default
//! and mixed weights. Acceptance: Rust matches within 1cp (target: ≪ 0.01cp).
use cvs_bitboard_core::eval::{evaluate_white_float, Rung2Weights, ValueWeights};
use cvs_bitboard_core::Position;
use serde::Deserialize;
use std::time::Instant;

#[derive(Deserialize)]
struct Fixture {
    #[serde(rename = "baseWeights")]
    base_weights: ValueWeights,
    #[serde(rename = "rung2Weights")]
    rung2_weights: Rung2Weights,
    positions: Vec<Case>,
}

#[derive(Deserialize)]
struct Case {
    fen: String,
    default: f64,
    mixed: f64,
}

fn main() {
    let path = std::env::args().nth(1).unwrap_or_else(|| {
        "../chess-vision-studio/arena/out/eval-parity-fixtures.json".to_string()
    });
    let raw = std::fs::read_to_string(&path).expect("read fixtures");
    let fx: Fixture = serde_json::from_str(&raw).expect("parse fixtures");
    let defaults = ValueWeights::default();

    println!("eval parity: {} positions from {}\n", fx.positions.len(), path);
    let mut max_def = 0.0f64;
    let mut max_mix = 0.0f64;
    let mut worst_def = String::new();
    let mut worst_mix = String::new();
    let mut over_1cp = 0u32;
    for c in &fx.positions {
        let mut pos = Position::from_fen(&c.fen).expect("valid FEN");
        let d = evaluate_white_float(&mut pos, &defaults, None);
        let m = evaluate_white_float(&mut pos, &fx.base_weights, Some(&fx.rung2_weights));
        let dd = (d - c.default).abs();
        let dm = (m - c.mixed).abs();
        if dd > max_def {
            max_def = dd;
            worst_def = c.fen.clone();
        }
        if dm > max_mix {
            max_mix = dm;
            worst_mix = c.fen.clone();
        }
        if dd > 1.0 || dm > 1.0 {
            over_1cp += 1;
            if over_1cp <= 5 {
                println!("MISMATCH {} | default rust {d:.4} ts {:.4} | mixed rust {m:.4} ts {:.4}", c.fen, c.default, c.mixed);
            }
        }
    }
    println!("| Weights | Max |Rust−TS| (cp) | Worst FEN |");
    println!("|---|---:|---|");
    println!("| default | {max_def:.6} | {} |", if max_def > 0.0 { worst_def.as_str() } else { "—" });
    println!("| mixed   | {max_mix:.6} | {} |", if max_mix > 0.0 { worst_mix.as_str() } else { "—" });
    println!("\npositions over 1cp tolerance: {over_1cp}");

    // Speed: pre-parse positions, then time evals (mixed = the expensive path).
    let mut parsed: Vec<Position> = fx.positions.iter().map(|c| Position::from_fen(&c.fen).unwrap()).collect();
    for (label, base, r2) in [
        ("default", &defaults, None::<&Rung2Weights>),
        ("mixed+rung2", &fx.base_weights, Some(&fx.rung2_weights)),
    ] {
        let iters = 20usize;
        let t = Instant::now();
        let mut sink = 0.0f64;
        for _ in 0..iters {
            for pos in parsed.iter_mut() {
                sink += evaluate_white_float(pos, base, r2);
            }
        }
        let secs = t.elapsed().as_secs_f64();
        let n = (iters * parsed.len()) as f64;
        println!("speed [{label}]: {:.0} evals/sec ({n:.0} evals in {secs:.3}s, sink {sink:.1})", n / secs);
    }

    if over_1cp == 0 {
        println!("\nEVAL PARITY OK (within 1cp everywhere; max diff {:.6}cp)", max_def.max(max_mix));
    } else {
        println!("\nEVAL PARITY FAILED — debug RUST (never patch the TS reference)");
        std::process::exit(1);
    }
}
