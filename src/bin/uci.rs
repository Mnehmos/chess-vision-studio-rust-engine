//! Minimal UCI front-end — the cutechess-cli / external-tournament interface.
//!
//! Supports the subset every match harness needs:
//!   uci / isready / ucinewgame / quit
//!   position (startpos | fen <fen>) [moves m1 m2 ...]
//!   go [movetime N] [depth D] [wtime N btime N winc N binc N] [infinite]
//!
//! Weights load exactly like `analyze`: --base w.json --rung2 r.json
//! (paths resolve from the cwd cutechess launches us in, so pass absolute
//! paths in the engine config). The clock policy mirrors the Lichess bot:
//! spend ~1/30 of remaining time + most of the increment, floor 50ms.
use cvs_bitboard_core::eval::{Rung2Weights, ValueWeights};
use cvs_bitboard_core::movegen::generate_legal;
use cvs_bitboard_core::search::{SearchOptions, Searcher};
use cvs_bitboard_core::Position;
use std::io::{BufRead, Write};

const START_FEN: &str = "rnbqkbnr/pppppppp/8/8/8/8/PPPPPPPP/RNBQKBNR w KQkq - 0 1";
const DEPTH_CAP: u32 = 30;

fn main() {
    let args: Vec<String> = std::env::args().collect();
    let get = |flag: &str| -> Option<String> {
        args.iter().position(|a| a == flag).and_then(|i| args.get(i + 1).cloned())
    };
    let base: ValueWeights = match get("--base") {
        Some(p) => serde_json::from_str(&std::fs::read_to_string(p).expect("base weights")).expect("parse base"),
        None => ValueWeights::default(),
    };
    let rung2: Option<Rung2Weights> = get("--rung2").map(|p| {
        serde_json::from_str(&std::fs::read_to_string(p).expect("rung2 weights")).expect("parse rung2")
    });

    let stdin = std::io::stdin();
    let mut out = std::io::stdout();
    let mut pos = Position::from_fen(START_FEN).unwrap();
    let mut searcher = Searcher::new(base, rung2);

    for line in stdin.lock().lines() {
        let line = match line {
            Ok(l) => l,
            Err(_) => break,
        };
        let mut tok = line.split_whitespace();
        match tok.next() {
            Some("uci") => {
                let _ = writeln!(out, "id name CVS Bitboard Core");
                let _ = writeln!(out, "id author Chess Vision Studio (MIT)");
                let _ = writeln!(out, "uciok");
            }
            Some("isready") => {
                let _ = writeln!(out, "readyok");
            }
            Some("ucinewgame") => {
                searcher = Searcher::new(base, rung2.clone());
                pos = Position::from_fen(START_FEN).unwrap();
            }
            Some("position") => {
                let rest: Vec<&str> = tok.collect();
                let (fen, moves_at) = match rest.first() {
                    Some(&"startpos") => (START_FEN.to_string(), 1),
                    Some(&"fen") => {
                        // FEN is the next 6 tokens (cutechess always sends all six).
                        let end = (1 + 6).min(rest.len());
                        (rest[1..end].join(" "), end)
                    }
                    _ => continue,
                };
                let Ok(mut p) = Position::from_fen(&fen) else { continue };
                let mut idx = moves_at;
                if rest.get(idx) == Some(&"moves") {
                    idx += 1;
                    while let Some(m) = rest.get(idx) {
                        let legal = generate_legal(&mut p);
                        match legal.iter().find(|mv| mv.to_uci() == *m) {
                            Some(&mv) => p.make(mv),
                            None => break, // illegal/unknown move: keep the last good position
                        }
                        idx += 1;
                    }
                }
                pos = p;
            }
            Some("go") => {
                let mut movetime: Option<u64> = None;
                let mut depth: Option<u32> = None;
                let mut wtime: Option<u64> = None;
                let mut btime: Option<u64> = None;
                let mut winc: u64 = 0;
                let mut binc: u64 = 0;
                let rest: Vec<&str> = tok.collect();
                let mut i = 0;
                while i < rest.len() {
                    let val = |j: usize| rest.get(j + 1).and_then(|v| v.parse::<u64>().ok());
                    match rest[i] {
                        "movetime" => movetime = val(i),
                        "depth" => depth = val(i).map(|v| v as u32),
                        "wtime" => wtime = val(i),
                        "btime" => btime = val(i),
                        "winc" => winc = val(i).unwrap_or(0),
                        "binc" => binc = val(i).unwrap_or(0),
                        _ => {
                            i += 1;
                            continue;
                        }
                    }
                    i += 2;
                }
                let white_to_move = pos.stm == cvs_bitboard_core::Color::White;
                let (my_time, my_inc) = if white_to_move { (wtime, winc) } else { (btime, binc) };
                let budget: Option<u64> = movetime.or_else(|| {
                    my_time.map(|t| ((t / 30 + my_inc * 4 / 5).clamp(50, 10_000)))
                });
                let opts = SearchOptions {
                    depth: depth.unwrap_or(DEPTH_CAP),
                    max_time_ms: if depth.is_some() { None } else { budget },
                    quiet_checks: true,
                    use_tt: true,
                    danger_extension: false,
                };
                let r = searcher.search(&mut pos, opts);
                let score = match r.mate {
                    Some(m) => format!("mate {m}"),
                    None => format!("cp {}", r.score_cp),
                };
                let pv: Vec<String> = r.pv.iter().map(|m| m.to_uci()).collect();
                let _ = writeln!(
                    out,
                    "info depth {} score {} nodes {} time {} pv {}",
                    r.depth,
                    score,
                    r.telemetry.nodes,
                    r.telemetry.elapsed_ms,
                    pv.join(" ")
                );
                match r.best_move {
                    Some(m) => {
                        let _ = writeln!(out, "bestmove {}", m.to_uci());
                    }
                    None => {
                        let _ = writeln!(out, "bestmove 0000");
                    }
                }
            }
            Some("quit") => break,
            Some("stop") => { /* searches are synchronous and bounded; nothing to stop */ }
            _ => {}
        }
        let _ = out.flush();
    }
}
