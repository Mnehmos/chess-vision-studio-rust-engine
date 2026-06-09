//! Batch analyze CLI — the engine-backend interface for the gate + gauntlet
//! harnesses. Two modes:
//!   --fens <file>   batch: search every FEN in the file, one JSON line each
//!   --serve         loop: read FEN per stdin line, emit one JSON line per FEN
//!                   (one process serves a whole gauntlet run)
//!
//!   analyze (--fens <file> | --serve) --depth N [--base w.json --rung2 r.json]
//!           [--no-quiet-checks] [--no-tt]
use cvs_bitboard_core::eval::{evaluate_white, Rung2Weights, ValueWeights};
use cvs_bitboard_core::search::{SearchOptions, Searcher};
use cvs_bitboard_core::Position;
use std::io::{BufRead, Write};

fn main() {
    let args: Vec<String> = std::env::args().collect();
    let get = |flag: &str| -> Option<String> {
        args.iter().position(|a| a == flag).and_then(|i| args.get(i + 1).cloned())
    };
    let serve = args.iter().any(|a| a == "--serve");
    let fens_path = get("--fens");
    if !serve && fens_path.is_none() {
        eprintln!("usage: analyze (--fens <file> | --serve) --depth N [--base w.json --rung2 r.json]");
        std::process::exit(2);
    }
    let depth: u32 = get("--depth").expect("--depth required").parse().expect("depth");
    let base: ValueWeights = match get("--base") {
        Some(p) => serde_json::from_str(&std::fs::read_to_string(p).expect("base weights")).expect("parse base"),
        None => ValueWeights::default(),
    };
    let rung2: Option<Rung2Weights> = get("--rung2").map(|p| {
        serde_json::from_str(&std::fs::read_to_string(p).expect("rung2 weights")).expect("parse rung2")
    });
    let opts = SearchOptions {
        depth,
        max_time_ms: None,
        quiet_checks: !args.iter().any(|a| a == "--no-quiet-checks"),
        use_tt: !args.iter().any(|a| a == "--no-tt"),
    };

    let analyze_one = |fen: &str| -> String {
        let mut pos = match Position::from_fen(fen) {
            Ok(p) => p,
            Err(e) => {
                return serde_json::json!({ "fen": fen, "error": e }).to_string();
            }
        };
        let mut searcher = Searcher::new(base, rung2);
        let r = searcher.search(&mut pos, opts);
        let t = r.telemetry;
        let uci = r.best_move.map(|m| m.to_uci());
        let pv: Vec<String> = r.pv.iter().map(|m| m.to_uci()).collect();
        serde_json::json!({
            "fen": fen,
            "uci": uci,
            "scoreCp": r.score_cp,
            "mate": r.mate,
            "pv": pv,
            "depth": r.depth,
            "nodes": t.nodes,
            "qNodes": t.q_nodes,
            "qCaptures": t.q_capture_nodes,
            "quietExt": t.quiet_check_extensions,
            "ttHits": t.tt_hits,
            "cutoffs": t.beta_cutoffs,
            "timeMs": t.elapsed_ms,
        })
        .to_string()
    };

    if serve {
        // Loop mode: one process serves a whole gauntlet run. Flush per line so
        // the orchestrator's request/response cycle never stalls on buffering.
        let stdin = std::io::stdin();
        let mut stdout = std::io::stdout();
        for line in stdin.lock().lines() {
            let line = match line {
                Ok(l) => l,
                Err(_) => break,
            };
            let fen = line.trim();
            if fen.is_empty() || fen == "quit" {
                if fen == "quit" {
                    break;
                }
                continue;
            }
            // `eval <fen>` → static eval only (White-POV rounded cp, TS-parity).
            let out = if let Some(efen) = fen.strip_prefix("eval ") {
                match Position::from_fen(efen.trim()) {
                    Ok(mut pos) => serde_json::json!({
                        "fen": efen.trim(),
                        "evalWhiteCp": evaluate_white(&mut pos, &base, rung2.as_ref()),
                    })
                    .to_string(),
                    Err(e) => serde_json::json!({ "fen": efen.trim(), "error": e }).to_string(),
                }
            } else {
                analyze_one(fen)
            };
            writeln!(stdout, "{out}").expect("stdout");
            stdout.flush().expect("flush");
        }
        return;
    }

    let fens = std::fs::read_to_string(fens_path.unwrap()).expect("read fens");
    for fen in fens.lines().map(str::trim).filter(|l| !l.is_empty()) {
        println!("{}", analyze_one(fen));
    }
}
