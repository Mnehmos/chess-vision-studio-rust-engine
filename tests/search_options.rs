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
