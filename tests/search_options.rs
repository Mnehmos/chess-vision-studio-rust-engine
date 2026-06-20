use cvs_bitboard_core::search::SearchOptions;
use serde_json::Value;
use std::io::Write;
use std::process::{Command, Stdio};

fn args(values: &[&str]) -> Vec<String> {
    values.iter().map(|value| (*value).to_owned()).collect()
}

#[test]
fn champion_defaults_keep_rejected_experiments_off() {
    let options = SearchOptions::default();
    assert!(options.rfp);
    assert!(options.futility);
    assert!(options.tt_prune_store);
    assert!(options.qsearch_tt);
    assert!(options.hist_malus);
    assert!(options.hist_lmr);

    assert!(!options.lmp);
    assert!(!options.see_prune);
    assert!(!options.delta_prune);
    assert!(!options.countermove);
    assert!(!options.conthist);
    assert!(!options.rule50_scale);
    assert!(!options.king_activity);
    assert!(!options.caphist);
    assert!(!options.tt2);
    assert!(!options.improving);
    assert!(!options.singular);
}

#[test]
fn cli_flags_opt_experiments_in_and_override_defaults_off() {
    let options = SearchOptions::default().with_cli_flags(&args(&[
        "--lmp",
        "--seeprune",
        "--delta",
        "--countermove",
        "--conthist",
        "--rule50",
        "--king-activity",
        "--caphist",
        "--tt2",
        "--improving",
        "--singular",
        "--no-rfp",
        "--no-futility",
    ]));

    assert!(options.lmp);
    assert!(options.see_prune);
    assert!(options.delta_prune);
    assert!(options.countermove);
    assert!(options.conthist);
    assert!(options.rule50_scale);
    assert!(options.king_activity);
    assert!(options.caphist);
    assert!(options.tt2);
    assert!(options.improving);
    assert!(options.singular);
    assert!(!options.rfp);
    assert!(!options.futility);
}

#[test]
fn analyze_identity_reports_effective_search_options() {
    let mut child = Command::new(env!("CARGO_BIN_EXE_analyze"))
        .args(["--serve", "--depth", "1", "--lmp", "--no-rfp"])
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .spawn()
        .unwrap();

    child
        .stdin
        .as_mut()
        .unwrap()
        .write_all(b"{\"cmd\":\"identity\"}\n")
        .unwrap();
    writeln!(child.stdin.as_mut().unwrap(), "quit").unwrap();
    let output = child.wait_with_output().unwrap();
    assert!(output.status.success());

    let line = String::from_utf8(output.stdout).unwrap();
    let value: Value = serde_json::from_str(line.lines().next().unwrap()).unwrap();
    assert_eq!(value["engine"], "cvs-bitboard-core");
    assert_eq!(value["depth"], 1);
    assert_eq!(value["options"]["lmp"], true);
    assert_eq!(value["options"]["rfp"], false);
    assert_eq!(value["options"]["futility"], true);
}

#[test]
fn analyze_search_reports_iteration_and_root_order_diagnostics() {
    let mut child = Command::new(env!("CARGO_BIN_EXE_analyze"))
        .args(["--serve", "--depth", "2"])
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .spawn()
        .unwrap();

    child
        .stdin
        .as_mut()
        .unwrap()
        .write_all(b"rnbqkbnr/pppppppp/8/8/8/8/PPPPPPPP/RNBQKBNR w KQkq - 0 1\n")
        .unwrap();
    writeln!(child.stdin.as_mut().unwrap(), "quit").unwrap();
    let output = child.wait_with_output().unwrap();
    assert!(output.status.success());

    let line = String::from_utf8(output.stdout).unwrap();
    let value: Value = serde_json::from_str(line.lines().next().unwrap()).unwrap();
    assert_eq!(value["iterations"].as_array().unwrap().len(), 2);
    assert_eq!(value["iterations"][1]["depth"], 2);
    assert!(!value["rootOrder"].as_array().unwrap().is_empty());
    assert_eq!(value["attemptedDepth"], 2);
    assert_eq!(value["termination"], "depth-limit");
    assert_eq!(value["resultSource"], "completed-iteration");
    assert!(value["partialIteration"].is_null());
}

#[test]
fn timed_search_returns_completed_iteration_and_reports_partial_root() {
    let mut child = Command::new(env!("CARGO_BIN_EXE_analyze"))
        .args([
            "--serve",
            "--depth",
            "30",
            "--no-book",
            "--no-syzygy",
            "--root-diagnostics",
        ])
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .spawn()
        .unwrap();

    child
        .stdin
        .as_mut()
        .unwrap()
        .write_all(
            b"{\"cmd\":\"go\",\"budgetMs\":10,\"fen\":\"4r3/2pk2pp/5p2/2P2b2/r7/3n1p2/P2B2PP/R4K1R w - - 0 32\"}\n",
        )
        .unwrap();
    writeln!(child.stdin.as_mut().unwrap(), "quit").unwrap();
    let output = child.wait_with_output().unwrap();
    assert!(output.status.success());

    let line = String::from_utf8(output.stdout).unwrap();
    let value: Value = serde_json::from_str(line.lines().next().unwrap()).unwrap();
    assert_eq!(value["termination"], "hard-time");
    assert_eq!(value["resultSource"], "completed-iteration");
    let iterations = value["iterations"].as_array().unwrap();
    let completed = iterations.last().expect("completed iteration");
    assert_eq!(value["uci"], completed["uci"]);
    assert_eq!(value["depth"], completed["depth"]);
    assert!(value["attemptedDepth"].as_u64().unwrap() > value["depth"].as_u64().unwrap());
    assert_eq!(value["partialIteration"]["depth"], value["attemptedDepth"],);
    assert!(
        value["partialIteration"]["completedCandidateCount"]
            .as_u64()
            .unwrap()
            <= value["partialIteration"]["totalCandidateCount"]
                .as_u64()
                .unwrap()
    );
}
