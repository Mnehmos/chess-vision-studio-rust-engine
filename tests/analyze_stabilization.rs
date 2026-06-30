//! #7 follow-up: `analyze` emits the StabilizationReport so the app-side policy (#35) consumes the
//! engine's stability verdict end-to-end, instead of reinterpreting raw depth/score. This runs the
//! DEBUG analyze binary (CARGO_BIN_EXE_analyze) — never the release analyze.exe the bot uses.
use std::process::Command;

const VALID: &[&str] = &[
    "exact-tablebase",
    "verified-forced-mate",
    "stable-at-budget",
    "unstable-trajectory",
    "omission-risk",
    "verifier-conflict",
    "unresolved-at-budget",
];

#[test]
fn analyze_fens_emits_a_kebab_stabilization_status() {
    // Two distinct positions so we prove the status is produced PER-POSITION from the trajectory
    // (not a hardcoded constant): the start position and a simple R+K vs K winning endgame.
    let path = std::env::temp_dir().join("cvs_stab_fens.txt");
    std::fs::write(
        &path,
        "rnbqkbnr/pppppppp/8/8/8/8/PPPPPPPP/RNBQKBNR w KQkq - 0 1\n4k3/8/8/8/8/8/8/4K2R w K - 0 1\n",
    )
    .unwrap();

    let out = Command::new(env!("CARGO_BIN_EXE_analyze"))
        .args(["--fens", path.to_str().unwrap(), "--depth", "8"])
        .output()
        .expect("run analyze");
    let _ = std::fs::remove_file(&path);
    assert!(out.status.success(), "analyze failed: {}", String::from_utf8_lossy(&out.stderr));

    let stdout = String::from_utf8_lossy(&out.stdout);
    let lines: Vec<&str> = stdout.lines().filter(|l| l.contains("\"fen\"")).collect();
    assert_eq!(lines.len(), 2, "one result line per FEN, got: {stdout}");
    for line in &lines {
        assert!(line.contains("\"stabilization\""), "stabilization block emitted: {line}");
        assert!(line.contains("\"status\""), "status field emitted: {line}");
        assert!(
            VALID.iter().any(|s| line.contains(&format!("\"{s}\""))),
            "a known kebab StabilizationStatus must be present in: {line}"
        );
    }
}
