//! Minimal UCI front-end — the cutechess-cli / external-tournament interface.
//!
//! Supports the subset every match harness needs:
//!   uci / isready / ucinewgame / quit
//!   position (startpos | fen <fen>) [moves m1 m2 ...]
//!   go [movetime N] [depth D] [wtime N btime N winc N binc N] [infinite] [ponder]
//!   ponderhit / stop
//!
//! Pondering (the promoted bot-layer design, single-prediction UCI form):
//! every bestmove carries `ponder <pv2>`; on `go ponder` the search runs on a
//! worker thread with no time limit while the main loop keeps reading stdin.
//! `ponderhit` arms the normal clock budget from that moment and the result
//! prints when the search lands; `stop` (ponder miss) aborts immediately.
//! The TT persists across searches, so even a missed ponder warms the next go.
//!
//! Weights load exactly like `analyze`: --base w.json --rung2 r.json
//! (paths resolve from the cwd cutechess launches us in, so pass absolute
//! paths in the engine config). The clock policy mirrors the Lichess bot:
//! spend ~1/30 of remaining time + most of the increment, floor 50ms.
use cvs_bitboard_core::eval::{Nnue, Rung2Weights, ValueWeights};
use cvs_bitboard_core::movegen::generate_legal;
use cvs_bitboard_core::search::{SearchOptions, SearchResult, Searcher};
use cvs_bitboard_core::Position;
use std::io::{BufRead, Write};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;

const START_FEN: &str = "rnbqkbnr/pppppppp/8/8/8/8/PPPPPPPP/RNBQKBNR w KQkq - 0 1";
const DEPTH_CAP: u32 = 30;

/// In-flight ponder search: worker owns the Searcher and hands it back.
struct Ponder {
    handle: std::thread::JoinHandle<(SearchResult, Searcher)>,
    stop: Arc<AtomicBool>,
    /// Clock budget (ms) to grant after ponderhit, from the go-ponder clocks.
    budget: u64,
}

fn print_result(out: &mut impl Write, r: &SearchResult, pos: &Position) {
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
            // The PV comes from a TT walk, so pv[1] can be stale (an entry
            // overwritten by a sibling search) and occasionally ILLEGAL after
            // the best move — GUIs warn and skip the hint. Validate it.
            let hint = r.pv.get(1).and_then(|p| {
                let mut after = pos.clone();
                let legal_best = generate_legal(&mut after)
                    .iter()
                    .any(|mv| mv.to_uci() == m.to_uci());
                if !legal_best {
                    return None;
                }
                after.make(m);
                generate_legal(&mut after)
                    .iter()
                    .find(|mv| mv.to_uci() == p.to_uci())
                    .map(|_| format!(" ponder {}", p.to_uci()))
            });
            let _ = writeln!(out, "bestmove {}{}", m.to_uci(), hint.unwrap_or_default());
        }
        None => {
            let _ = writeln!(out, "bestmove 0000");
        }
    }
}

fn main() {
    let args: Vec<String> = std::env::args().collect();
    let get = |flag: &str| -> Option<String> {
        args.iter()
            .position(|a| a == flag)
            .and_then(|i| args.get(i + 1).cloned())
    };
    let base: ValueWeights = match get("--base") {
        Some(p) => serde_json::from_str(&std::fs::read_to_string(p).expect("base weights"))
            .expect("parse base"),
        None => ValueWeights::default(),
    };
    let rung2: Option<Rung2Weights> = get("--rung2").map(|p| {
        serde_json::from_str(&std::fs::read_to_string(p).expect("rung2 weights"))
            .expect("parse rung2")
    });

    let allow_unverified = args.iter().any(|a| a == "--allow-unverified-net");
    let nnue: Option<Nnue> =
        get("--nnue").map(|p| Nnue::load(&p, allow_unverified).expect("load nnue"));
    let helper_nnue: Option<Nnue> =
        get("--helper-nnue").map(|p| Nnue::load(&p, allow_unverified).expect("load helper nnue"));
    let syzygy_path = get("--syzygy");
    let book_path = get("--book");
    let mk = |base: ValueWeights, rung2: Option<Rung2Weights>| {
        let mut searcher = match &nnue {
            Some(n) => Searcher::with_nnue(base, rung2, n.clone()),
            None => Searcher::new(base, rung2),
        };
        if let Some(n) = &helper_nnue {
            searcher.set_helper_nnue(Some(n.clone()));
        }
        if let Some(path) = &syzygy_path {
            if let Ok(tb) = cvs_bitboard_core::syzygy::Syzygy::new(path) {
                searcher.tb = Some(Arc::new(tb));
            }
        }
        if let Some(path) = &book_path {
            if let Ok(b) = cvs_bitboard_core::book::Book::new(path) {
                searcher.book = Some(Arc::new(std::sync::Mutex::new(b)));
            }
        }
        searcher
    };
    let stdin = std::io::stdin();
    let mut out = std::io::stdout();
    let mut pos = Position::from_fen(START_FEN).unwrap();
    let mut searcher: Option<Searcher> = Some(mk(base, rung2));
    let mut ponder: Option<Ponder> = None;

    // Abort an in-flight ponder without printing (position/newgame/quit while
    // pondering — shouldn't happen with a conforming GUI, but never wedge).
    macro_rules! abort_ponder {
        () => {
            if let Some(p) = ponder.take() {
                p.stop.store(true, Ordering::Relaxed);
                if let Ok((_, s)) = p.handle.join() {
                    searcher = Some(s);
                }
            }
        };
    }

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
                let _ = writeln!(out, "option name Ponder type check default false");
                let _ = writeln!(out, "option name SyzygyPath type string default <empty>");
                let _ = writeln!(out, "option name BookPath type string default <empty>");
                let _ = writeln!(out, "uciok");
            }
            Some("isready") => {
                let _ = writeln!(out, "readyok");
            }
            Some("setoption") => {
                let rest: Vec<&str> = tok.collect();
                if rest.get(0) == Some(&"name") {
                    let mut name_idx = 1;
                    while name_idx < rest.len() && rest[name_idx] != "value" {
                        name_idx += 1;
                    }
                    let name = rest[1..name_idx].join(" ");
                    if name_idx < rest.len() {
                        let value = rest[name_idx + 1..].join(" ");
                        if name == "SyzygyPath" {
                            if !value.is_empty() && value != "<empty>" {
                                if let Ok(tb) = cvs_bitboard_core::syzygy::Syzygy::new(&value) {
                                    if let Some(s) = &mut searcher {
                                        s.tb = Some(Arc::new(tb));
                                    }
                                }
                            }
                        } else if name == "BookPath" {
                            if !value.is_empty() && value != "<empty>" {
                                if let Ok(b) = cvs_bitboard_core::book::Book::new(&value) {
                                    if let Some(s) = &mut searcher {
                                        s.book = Some(Arc::new(std::sync::Mutex::new(b)));
                                    }
                                }
                            }
                        }
                    }
                }
            }
            Some("ucinewgame") => {
                abort_ponder!();
                searcher = Some(mk(base, rung2.clone()));
                pos = Position::from_fen(START_FEN).unwrap();
            }
            Some("position") => {
                abort_ponder!();
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
                let Ok(mut p) = Position::from_fen(&fen) else {
                    continue;
                };
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
                let mut pondering = false;
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
                        "ponder" => {
                            pondering = true;
                            i += 1;
                            continue;
                        }
                        _ => {
                            i += 1;
                            continue;
                        }
                    }
                    i += 2;
                }
                let white_to_move = pos.stm == cvs_bitboard_core::Color::White;
                let (my_time, my_inc) = if white_to_move {
                    (wtime, winc)
                } else {
                    (btime, binc)
                };
                let budget: Option<u64> = movetime
                    .or_else(|| my_time.map(|t| ((t / 30 + my_inc * 4 / 5).clamp(50, 10_000))));
                // --smarttime: soft/hard split instead of the flat budget. Soft
                // is the iteration-boundary target; hard caps runaway thinks
                // and is enforced by the existing mid-search deadline.
                let smart = args.iter().any(|a| a == "--smarttime") && movetime.is_none();
                let (soft, hard) = if smart {
                    let t = my_time.unwrap_or(1_000);
                    let soft = (t / 25 + my_inc * 3 / 4).clamp(30, 8_000);
                    let hard = (soft * 4).min(t / 6).max(soft).min(12_000);
                    (Some(soft), Some(hard))
                } else {
                    (None, budget)
                };
                let opts = SearchOptions {
                    depth: depth.unwrap_or(DEPTH_CAP),
                    max_time_ms: if depth.is_some() || pondering {
                        None
                    } else {
                        hard
                    },
                    soft_time_ms: if depth.is_some() || pondering {
                        None
                    } else {
                        soft
                    },
                    threads: get("--threads").and_then(|s| s.parse().ok()).unwrap_or(1),
                    ..Default::default()
                }
                .with_cli_flags(&args);
                if pondering {
                    // Opponent-clock search: free depth, no bestmove until
                    // ponderhit (clock arms) or stop (miss; result discarded
                    // by the GUI, TT keeps the work).
                    let stop = Arc::new(AtomicBool::new(false));
                    let mut s = searcher.take().expect("searcher");
                    let mut p = pos.clone();
                    let st = Arc::clone(&stop);
                    let handle = std::thread::spawn(move || {
                        s.set_stop(Some(st));
                        let r = s.search(&mut p, opts);
                        s.set_stop(None);
                        (r, s)
                    });
                    ponder = Some(Ponder {
                        handle,
                        stop,
                        budget: budget.unwrap_or(1_000),
                    });
                } else {
                    let s = searcher.as_mut().expect("searcher");
                    let r = s.search(&mut pos, opts);
                    print_result(&mut out, &r, &pos);
                }
            }
            Some("ponderhit") => {
                if let Some(p) = ponder.take() {
                    // Prediction confirmed: grant the normal budget from now.
                    let stop = Arc::clone(&p.stop);
                    let ms = p.budget;
                    std::thread::spawn(move || {
                        std::thread::sleep(std::time::Duration::from_millis(ms));
                        stop.store(true, Ordering::Relaxed);
                    });
                    if let Ok((r, s)) = p.handle.join() {
                        searcher = Some(s);
                        print_result(&mut out, &r, &pos);
                    }
                }
            }
            Some("stop") => {
                if let Some(p) = ponder.take() {
                    p.stop.store(true, Ordering::Relaxed);
                    if let Ok((r, s)) = p.handle.join() {
                        searcher = Some(s);
                        // Protocol requires a bestmove even on ponder miss.
                        print_result(&mut out, &r, &pos);
                    }
                }
                // Non-ponder searches are synchronous and bounded; nothing to stop.
            }
            Some("quit") => {
                abort_ponder!();
                break;
            }
            _ => {}
        }
        let _ = out.flush();
    }
}
