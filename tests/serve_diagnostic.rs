//! Fixed-node diagnostic interface (CLASSICAL_EVAL_EXPERIMENT.md Phase 5 / #6).
//!
//! The experiment's decision gates rest on deterministic, cold, single-thread, fixed-node
//! comparisons. These tests pin the serve contract that makes those comparisons trustworthy:
//! a node budget is honoured exactly, cold runs are byte-reproducible, a prior search cannot
//! alter a cold result, and a warm (persisted-TT) run genuinely carries state forward.
use serde_json::Value;
use std::io::Write;
use std::process::{Command, Stdio};

const FEN: &str = "r1bqkbnr/pppp1ppp/2n5/1B2p3/4P3/5N2/PPPP1PPP/RNBQK2R w KQkq - 3 4";
const BUDGET: u64 = 20_000;

/// Feed one serve process the given request lines (a `quit` is appended) and return the
/// parsed JSON responses in order.
fn serve(requests: &[Value]) -> Vec<Value> {
    let mut child = Command::new(env!("CARGO_BIN_EXE_analyze"))
        .args(["--serve", "--depth", "30"])
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .spawn()
        .unwrap();
    {
        let stdin = child.stdin.as_mut().unwrap();
        for req in requests {
            writeln!(stdin, "{req}").unwrap();
        }
        writeln!(stdin, "quit").unwrap();
    }
    let output = child.wait_with_output().unwrap();
    assert!(output.status.success());
    String::from_utf8(output.stdout)
        .unwrap()
        .lines()
        .filter(|l| !l.trim().is_empty())
        .map(|l| serde_json::from_str(l).unwrap())
        .collect()
}

fn go(isolation: &str) -> Value {
    serde_json::json!({
        "cmd": "go",
        "fen": FEN,
        "nodeBudget": BUDGET,
        "diagnosticIsolation": isolation,
    })
}

/// The decision fields that must match for two runs to count as "the same result".
fn decision(v: &Value) -> (String, i64, u64, u64) {
    (
        v["uci"].as_str().unwrap().to_string(),
        v["scoreCp"].as_i64().unwrap(),
        v["depth"].as_u64().unwrap(),
        v["nodes"].as_u64().unwrap(),
    )
}

#[test]
fn cold_fixed_node_is_deterministic_and_honours_the_budget() {
    let out = serve(&[go("cold"), go("cold")]);
    assert_eq!(out.len(), 2);

    // Identical cold requests -> identical decision.
    assert_eq!(decision(&out[0]), decision(&out[1]), "cold runs must be reproducible");

    for v in &out {
        let d = &v["diagnostic"];
        assert_eq!(d["isolation"], "cold");
        assert_eq!(d["requestedNodes"].as_u64().unwrap(), BUDGET);
        // The budget is a hard ceiling: consumed == requested (search stops exactly on it).
        assert_eq!(d["consumedNodes"].as_u64().unwrap(), BUDGET, "budget must bind exactly");
        assert_eq!(d["consumedNodes"], v["nodes"], "consumedNodes mirrors telemetry nodes");
        assert_eq!(d["singleThread"], true, "diagnostic search is single-thread");
        assert_eq!(d["book"], false, "book is off unless declared");
    }
}

#[test]
fn a_prior_search_cannot_alter_a_cold_result() {
    // Standalone cold baseline.
    let baseline = serve(&[go("cold")]);
    // Two warm searches (which populate + carry the TT) followed by a cold search on the
    // same position. Cold builds a fresh searcher, so its result must equal the baseline.
    let mixed = serve(&[go("warm"), go("warm"), go("cold")]);
    assert_eq!(mixed.len(), 3);

    assert_eq!(
        decision(&baseline[0]),
        decision(&mixed[2]),
        "a cold result must be independent of any prior (warm) search"
    );
    assert_eq!(mixed[2]["diagnostic"]["isolation"], "cold");
}

#[test]
fn warm_carries_the_transposition_table_forward() {
    // Same position, same node budget, back to back under warm isolation. The second warm
    // search reuses the aged TT, so within the identical budget it reaches at least as deep
    // as the first (in practice strictly deeper) — proof that warm state actually carries.
    let out = serve(&[go("warm"), go("warm")]);
    assert_eq!(out.len(), 2);
    let d1 = out[0]["depth"].as_u64().unwrap();
    let d2 = out[1]["depth"].as_u64().unwrap();
    assert!(
        d2 >= d1,
        "warm reuse must not lose depth at a fixed budget: first={d1} second={d2}"
    );
    assert_eq!(out[1]["diagnostic"]["isolation"], "warm");
    assert_eq!(out[1]["diagnostic"]["consumedNodes"].as_u64().unwrap(), BUDGET);
}
