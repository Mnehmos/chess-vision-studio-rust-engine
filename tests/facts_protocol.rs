use cvs_bitboard_core::facts::{
    build_teaching_fact_bundle, TeachingFactsOptionsV1, TeachingFactsRequestV1,
    TEACHING_FACTS_SCHEMA_VERSION,
};
use serde_json::Value;
use std::io::Write;
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};

fn request(
    fen: &str,
    played: &str,
    best: Option<&str>,
    refutation: Option<&str>,
) -> TeachingFactsRequestV1 {
    TeachingFactsRequestV1 {
        schema_version: TEACHING_FACTS_SCHEMA_VERSION,
        fen_before: fen.into(),
        played_move_uci: played.into(),
        best_move_uci: best.map(str::to_string),
        refutation_uci: refutation.map(str::to_string),
        principal_variation_uci: None,
        options: None,
    }
}

fn request_with_motifs(
    fen: &str,
    played: &str,
    best: Option<&str>,
    refutation: Option<&str>,
) -> TeachingFactsRequestV1 {
    TeachingFactsRequestV1 {
        options: Some(TeachingFactsOptionsV1 {
            include_motif_opportunities: true,
            include_counterfactual: true,
        }),
        ..request(fen, played, best, refutation)
    }
}

fn fixtures() -> Vec<(&'static str, TeachingFactsRequestV1)> {
    vec![
        (
            // White's Kg1/Re1 are forkable by Black's knight: after the quiet e2e4,
            // Black plays g5f3+ forking king and rook. Kg1-h1 (best) sidesteps it.
            "allowed-fork.json",
            request_with_motifs(
                "6k1/8/8/6n1/8/8/4P3/4R1K1 w - - 0 1",
                "e2e4",
                Some("g1h1"),
                Some("g5f3"),
            ),
        ),
        (
            "allowed-pin.json",
            request(
                "4k3/8/8/8/8/8/2B1P3/4K3 w - - 0 1",
                "e2e3",
                Some("c2d3"),
                Some("e8f7"),
            ),
        ),
        (
            "missed-hanging-piece.json",
            request(
                "4k3/8/8/8/4q3/8/4R3/4K3 w - - 0 1",
                "e1f1",
                Some("e2e4"),
                Some("e4e2"),
            ),
        ),
        (
            "failed-defense.json",
            request(
                "4k3/8/8/8/8/1b6/2R5/4K3 w - - 0 1",
                "e1f2",
                Some("c2c3"),
                Some("b3c2"),
            ),
        ),
        (
            "pawn-structure-damage.json",
            request(
                "4k3/8/8/8/2p5/1P6/2P5/4K3 w - - 0 1",
                "b3c4",
                Some("c2c3"),
                Some("e8f7"),
            ),
        ),
    ]
}

fn fixture_dir() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("fixtures/teaching-facts/v1")
}

#[test]
fn golden_fixtures_are_byte_stable() {
    let dir = fixture_dir();
    if std::env::var_os("UPDATE_TEACHING_FIXTURES").is_some() {
        std::fs::create_dir_all(&dir).unwrap();
    }
    for (name, request) in fixtures() {
        let bundle = build_teaching_fact_bundle(&request).unwrap();
        let actual = format!("{}\n", serde_json::to_string_pretty(&bundle).unwrap());
        let path = dir.join(name);
        if std::env::var_os("UPDATE_TEACHING_FIXTURES").is_some() {
            std::fs::write(&path, &actual).unwrap();
        }
        let expected = std::fs::read_to_string(&path)
            .unwrap_or_else(|error| panic!("read {}: {error}", path.display()));
        assert_eq!(actual, expected, "fixture drift: {}", path.display());
    }
}

#[test]
fn optional_illegal_moves_are_reported_without_destroying_played_facts() {
    let mut req = request(
        "4k3/8/8/8/8/8/4P3/4K3 w - - 0 1",
        "e2e4",
        Some("e2e5"),
        None,
    );
    req.principal_variation_uci = Some(vec!["e2e4".into(), "e8e9".into()]);
    let bundle = build_teaching_fact_bundle(&req).unwrap();
    assert_eq!(bundle.played.r#move.uci, "e2e4");
    assert!(bundle.best.is_none());
    assert_eq!(bundle.errors.len(), 2);
}

#[test]
fn serve_mode_accepts_the_distinct_facts_command() {
    let mut child = Command::new(env!("CARGO_BIN_EXE_analyze"))
        .args(["--serve", "--depth", "1"])
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .spawn()
        .unwrap();
    let command = serde_json::json!({
        "cmd": "facts",
        "schemaVersion": 1,
        "fenBefore": "4k3/8/8/8/8/8/4P3/4K3 w - - 0 1",
        "playedMoveUci": "e2e4"
    });
    writeln!(child.stdin.as_mut().unwrap(), "{command}").unwrap();
    writeln!(child.stdin.as_mut().unwrap(), "quit").unwrap();
    let output = child.wait_with_output().unwrap();
    assert!(output.status.success());
    let line = String::from_utf8(output.stdout).unwrap();
    let value: Value = serde_json::from_str(line.lines().next().unwrap()).unwrap();
    assert_eq!(value["schemaVersion"], 1);
    assert_eq!(value["played"]["move"]["uci"], "e2e4");
}
