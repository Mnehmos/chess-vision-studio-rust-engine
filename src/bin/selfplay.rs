//! Self-play data generator for NNUE training.
//!
//! Plays fixed-depth games from randomized openings and emits one JSONL row
//! per quiet position: `{"fen": ..., "cp": <white-POV search score>, "res":
//! <1|0.5|0 white result>}`. Search score + final result are the standard
//! NNUE label pair (the trainer blends them).
//!
//!   selfplay --games N --depth D --out file.jsonl [--threads T] [--seed S]
//!            [--base w.json --rung2 r.json]
//!
//! Opening: 8 random legal plies, rejected unless |eval| < 300cp at the end
//! (keeps openings playable). Adjudication: resign when |score| > 900cp for 4
//! consecutive plies, draw on 3-fold / 50-move / insufficient material /
//! 250-ply cap. Positions in check and the random opening plies are skipped
//! on output.
use cvs_bitboard_core::eval::{Rung2Weights, ValueWeights};
use cvs_bitboard_core::movegen::{generate_legal, in_check};
use cvs_bitboard_core::search::{SearchOptions, Searcher, MATE_THRESHOLD};
use cvs_bitboard_core::{Color, Position};
use std::io::Write;
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::Mutex;

struct Rng(u64);
impl Rng {
    fn next(&mut self) -> u64 {
        self.0 = self.0.wrapping_add(0x9E37_79B9_7F4A_7C15);
        let mut z = self.0;
        z = (z ^ (z >> 30)).wrapping_mul(0xBF58_476D_1CE4_E5B9);
        z = (z ^ (z >> 27)).wrapping_mul(0x94D0_49BB_1331_11EB);
        z ^ (z >> 31)
    }
    fn below(&mut self, n: usize) -> usize {
        (self.next() % n as u64) as usize
    }
}

const START_FEN: &str = "rnbqkbnr/pppppppp/8/8/8/8/PPPPPPPP/RNBQKBNR w KQkq - 0 1";

fn main() {
    let args: Vec<String> = std::env::args().collect();
    let get = |flag: &str| -> Option<String> {
        args.iter()
            .position(|a| a == flag)
            .and_then(|i| args.get(i + 1).cloned())
    };
    let games: u64 = get("--games").and_then(|s| s.parse().ok()).unwrap_or(1000);
    let depth: u32 = get("--depth").and_then(|s| s.parse().ok()).unwrap_or(6);
    let threads: usize = get("--threads").and_then(|s| s.parse().ok()).unwrap_or(12);
    let seed: u64 = get("--seed")
        .and_then(|s| s.parse().ok())
        .unwrap_or(0xC0FFEE);
    let out_path = get("--out").expect("--out required");
    let base: ValueWeights = match get("--base") {
        Some(p) => {
            serde_json::from_str(&std::fs::read_to_string(p).expect("base")).expect("parse base")
        }
        None => ValueWeights::default(),
    };
    let rung2: Option<Rung2Weights> = get("--rung2").map(|p| {
        serde_json::from_str(&std::fs::read_to_string(p).expect("rung2")).expect("parse rung2")
    });

    let file = Mutex::new(std::io::BufWriter::new(
        std::fs::File::create(&out_path).expect("create out"),
    ));
    let played = AtomicU64::new(0);
    let positions = AtomicU64::new(0);
    let started = std::time::Instant::now();

    std::thread::scope(|scope| {
        for t in 0..threads {
            let file = &file;
            let played = &played;
            let positions = &positions;
            let base = base;
            let rung2 = rung2;
            scope.spawn(move || {
                let mut rng = Rng(seed ^ (t as u64).wrapping_mul(0x9E37_79B9));
                let mut searcher = Searcher::new(base, rung2);
                loop {
                    let g = played.fetch_add(1, Ordering::Relaxed);
                    if g >= games {
                        played.fetch_sub(1, Ordering::Relaxed);
                        break;
                    }
                    if let Some((rows, n)) = play_one(&mut searcher, &mut rng, depth) {
                        positions.fetch_add(n, Ordering::Relaxed);
                        let mut f = file.lock().unwrap();
                        f.write_all(rows.as_bytes()).unwrap();
                    }
                    if g % 200 == 0 && g > 0 {
                        let secs = started.elapsed().as_secs_f64();
                        eprintln!(
                            "{g} games, {} positions, {:.1} games/min",
                            positions.load(Ordering::Relaxed),
                            g as f64 / (secs / 60.0)
                        );
                    }
                }
            });
        }
    });
    file.lock().unwrap().flush().unwrap();
    eprintln!(
        "done: {} games, {} positions in {:.0}s",
        played.load(Ordering::Relaxed),
        positions.load(Ordering::Relaxed),
        started.elapsed().as_secs_f64()
    );
}

/// Plays one game; returns (jsonl rows, row count) or None if the random
/// opening was rejected.
fn play_one(searcher: &mut Searcher, rng: &mut Rng, depth: u32) -> Option<(String, u64)> {
    let mut pos = Position::from_fen(START_FEN).unwrap();
    // Random opening: 8 plies.
    for _ in 0..8 {
        let legal = generate_legal(&mut pos);
        if legal.is_empty() {
            return None;
        }
        pos.make(legal[rng.below(legal.len())]);
    }
    let opts = SearchOptions {
        depth,
        ..Default::default()
    };
    let mut rows = String::new();
    let mut n_rows: u64 = 0;
    let mut fens: Vec<(String, i32)> = Vec::with_capacity(160);
    let mut resign_streak = 0i32;
    let mut hashes: Vec<u64> = vec![pos.hash];
    let result: f64;
    // Opening sanity probe.
    {
        let r = searcher.search(&mut pos.clone(), opts);
        if r.score_cp.abs() > 300 {
            return None;
        }
    }
    loop {
        let legal = generate_legal(&mut pos);
        if legal.is_empty() {
            result = if in_check(&pos) {
                if pos.stm == Color::White {
                    0.0
                } else {
                    1.0
                }
            } else {
                0.5
            };
            break;
        }
        if pos.halfmove >= 100
            || cvs_bitboard_core::eval::insufficient_material(&pos)
            || hashes.iter().filter(|&&h| h == pos.hash).count() >= 3
            || hashes.len() > 250
        {
            result = 0.5;
            break;
        }
        let r = searcher.search(&mut pos.clone(), opts);
        let Some(mv) = r.best_move else {
            result = 0.5;
            break;
        };
        let white_cp = if pos.stm == Color::White {
            r.score_cp
        } else {
            -r.score_cp
        };
        if white_cp.abs() < MATE_THRESHOLD && !in_check(&pos) {
            fens.push((pos.to_fen(), white_cp));
        }
        if r.score_cp > 900 || r.score_cp < -900 {
            resign_streak += 1;
            if resign_streak >= 4 {
                // Side to move is winning (>900) or losing (<-900).
                let stm_winning = r.score_cp > 900;
                result = match (pos.stm, stm_winning) {
                    (Color::White, true) | (Color::Black, false) => 1.0,
                    _ => 0.0,
                };
                break;
            }
        } else {
            resign_streak = 0;
        }
        pos.make(mv);
        hashes.push(pos.hash);
    }
    for (fen, cp) in fens {
        rows.push_str(&format!(
            "{{\"fen\":\"{fen}\",\"cp\":{cp},\"res\":{result}}}\n"
        ));
        n_rows += 1;
    }
    Some((rows, n_rows))
}
