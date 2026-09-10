//! Syzygy tablebase load + probe regression test.
//!
//! Guards the 2026-09-10 findings: pyrrhic-rs treats the path as a colon-separated list
//! (a Windows drive-letter path splits at the drive colon and only resolves
//! drive-relative, so the constructor can report success while probing nothing), and its
//! init is PROCESS-GLOBAL, so a second `TableBases::new` returns `AlreadyInitialized` —
//! both binaries now create one handle and share it into every rebuilt searcher.
//!
//! One test on purpose: the global init makes parallel tests in this file racy.
//! Skips when the tables are not on this machine (CI runs on Linux).
use cvs_bitboard_core::syzygy::Syzygy;
use cvs_bitboard_core::Position;

#[test]
fn syzygy_loads_probes_and_rejects_junk() {
    let path = std::env::var("CVS_SYZYGY_PATH")
        .unwrap_or_else(|_| "F:/tablebases/syzygy345".to_string());
    if !std::path::Path::new(&path).exists() {
        eprintln!("skipping: {path} not present on this machine");
        return;
    }
    let tb = Syzygy::new(&path).expect("Syzygy::new must load and probe");
    let pos = Position::from_fen("8/8/8/8/8/2k5/8/KQ6 w - - 0 1").expect("fen");
    assert!(
        tb.probe_wdl(&pos).is_some(),
        "KQvK must probe once the tables load"
    );
    // A junk path must never report success. (pyrrhic's init is process-global, so this
    // second call may instead surface AlreadyInitialized — either way it must be an Err.)
    assert!(Syzygy::new("Q:/definitely-not-a-tablebase-dir").is_err());
}
