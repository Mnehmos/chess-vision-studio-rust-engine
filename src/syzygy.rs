use crate::{Color, Piece, Position};
use pyrrhic_rs::{Color as PyrrhicColor, Piece as PyrrhicPiece, TableBases, WdlProbeResult};

#[derive(Clone, Copy)]
pub struct CvsEngineAdapter;

impl pyrrhic_rs::EngineAdapter for CvsEngineAdapter {
    fn pawn_attacks(color: PyrrhicColor, square: u64) -> u64 {
        let cvs_color = match color {
            PyrrhicColor::White => Color::White,
            PyrrhicColor::Black => Color::Black,
        };
        crate::attacks::pawn_attacks(cvs_color, square as u8)
    }

    fn knight_attacks(square: u64) -> u64 {
        crate::attacks::knight_attacks(square as u8)
    }

    fn bishop_attacks(square: u64, occupied: u64) -> u64 {
        crate::attacks::bishop_attacks(square as u8, occupied)
    }

    fn rook_attacks(square: u64, occupied: u64) -> u64 {
        crate::attacks::rook_attacks(square as u8, occupied)
    }

    fn queen_attacks(square: u64, occupied: u64) -> u64 {
        crate::attacks::queen_attacks(square as u8, occupied)
    }

    fn king_attacks(square: u64) -> u64 {
        crate::attacks::king_attacks(square as u8)
    }
}

pub struct Syzygy {
    pub tb: TableBases<CvsEngineAdapter>,
}

/// KQvK, White to move: a tablebase win, used to verify that tables really probe.
const PROBE_FEN: &str = "8/8/8/8/8/2k5/8/KQ6 w - - 0 1";

/// Split a Windows drive-letter prefix off a path (`F:/x` -> (`F`, `/x`)).
#[cfg(windows)]
fn split_drive_prefix(path: &str) -> Option<(&str, &str)> {
    let b = path.as_bytes();
    if b.len() >= 2 && b[1] == b':' && b[0].is_ascii_alphabetic() {
        Some((&path[..1], &path[2..]))
    } else {
        None
    }
}

#[cfg(not(windows))]
fn split_drive_prefix(_path: &str) -> Option<(&str, &str)> {
    None
}

/// The drive letter the process is running from, if any.
fn cwd_drive() -> Option<String> {
    let cwd = std::env::current_dir().ok()?;
    let s = cwd.to_string_lossy();
    split_drive_prefix(&s).map(|(d, _)| d.to_ascii_uppercase())
}

/// Rewrite a tablebase path into the form pyrrhic-rs can actually consume (see `Syzygy::new`).
fn normalize_tb_path(path: &str) -> String {
    if let Some((drive, rest)) = split_drive_prefix(path) {
        let same_drive = cwd_drive()
            .map(|c| c.eq_ignore_ascii_case(drive))
            .unwrap_or(false);
        if same_drive {
            // Drive-relative: the crate's colon split already produces this component.
            return rest.to_string();
        }
    }
    path.to_string()
}

/// Extra guidance for the error when a drive mismatch is the most likely cause.
fn tb_drive_hint(path: &str) -> String {
    if let Some((drive, _)) = split_drive_prefix(path) {
        if let Some(cwd) = cwd_drive() {
            if !cwd.eq_ignore_ascii_case(drive) {
                return format!(
                    " — pyrrhic-rs splits paths on ':' and resolves the remainder \
                     drive-relative, so tables on drive {drive}: cannot be reached from a \
                     working directory on drive {cwd}:. Run the engine from drive {drive}: \
                     (or move the tables)."
                );
            }
        }
    }
    String::new()
}

impl Syzygy {
    /// Load the Syzygy tables from `path`.
    ///
    /// pyrrhic-rs interprets the path as a **colon-separated list** (Unix PATH style), so a
    /// Windows drive-letter path like `F:/tablebases/syzygy345` is split at its drive colon
    /// into the components `"F"` and `"/tablebases/syzygy345"`. The second component then
    /// resolves *drive-relative*, which means the tables load only when the process working
    /// directory happens to be on the same drive as the tables — and fail silently
    /// otherwise. When the drive matches the cwd's drive, drop the drive prefix and use the
    /// drive-relative form deliberately; when it does not, return a loud error that names
    /// both drives, because no colon-free absolute path exists on Windows.
    pub fn new(path: &str) -> Result<Self, String> {
        let effective = normalize_tb_path(path);
        match TableBases::new(&effective) {
            Ok(tb) => {
                let syz = Syzygy { tb };
                // pyrrhic can report a successful init while resolving NO files (e.g. the
                // drive-relative path above lands on the wrong drive): max_pieces() still
                // reads the compiled-in maximum, so the only trustworthy check is a probe.
                // KQvK with White to move is a tablebase win.
                let probe_ok = Position::from_fen(PROBE_FEN)
                    .map(|p| syz.probe_wdl(&p).is_some())
                    .unwrap_or(false);
                if probe_ok {
                    Ok(syz)
                } else {
                    Err(format!(
                        "tablebases at {path:?} loaded but do not probe (used {effective:?}){}",
                        tb_drive_hint(path)
                    ))
                }
            }
            Err(e) => Err(format!(
                "Failed to initialize tablebases from {path:?} (used {effective:?}): {e:?}{}",
                tb_drive_hint(path)
            )),
        }
    }

    pub fn max_pieces(&self) -> u32 {
        self.tb.max_pieces()
    }

    pub fn probe_wdl(&self, pos: &Position) -> Option<WdlProbeResult> {
        if pos.castling != 0 {
            return None;
        }
        let white = pos.occ[Color::White.index()];
        let black = pos.occ[Color::Black.index()];
        let kings = pos.pieces[0][Piece::King.index()] | pos.pieces[1][Piece::King.index()];
        let queens = pos.pieces[0][Piece::Queen.index()] | pos.pieces[1][Piece::Queen.index()];
        let rooks = pos.pieces[0][Piece::Rook.index()] | pos.pieces[1][Piece::Rook.index()];
        let bishops = pos.pieces[0][Piece::Bishop.index()] | pos.pieces[1][Piece::Bishop.index()];
        let knights = pos.pieces[0][Piece::Knight.index()] | pos.pieces[1][Piece::Knight.index()];
        let pawns = pos.pieces[0][Piece::Pawn.index()] | pos.pieces[1][Piece::Pawn.index()];
        let ep = pos.ep.map(|s| s as u32).unwrap_or(0);
        let turn = pos.stm == Color::White;

        self.tb.probe_wdl(white, black, kings, queens, rooks, bishops, knights, pawns, ep, turn).ok()
    }

    pub fn probe_root(&self, pos: &Position) -> Option<(crate::Move, WdlProbeResult)> {
        if pos.castling != 0 {
            return None;
        }
        let white = pos.occ[Color::White.index()];
        let black = pos.occ[Color::Black.index()];
        let kings = pos.pieces[0][Piece::King.index()] | pos.pieces[1][Piece::King.index()];
        let queens = pos.pieces[0][Piece::Queen.index()] | pos.pieces[1][Piece::Queen.index()];
        let rooks = pos.pieces[0][Piece::Rook.index()] | pos.pieces[1][Piece::Rook.index()];
        let bishops = pos.pieces[0][Piece::Bishop.index()] | pos.pieces[1][Piece::Bishop.index()];
        let knights = pos.pieces[0][Piece::Knight.index()] | pos.pieces[1][Piece::Knight.index()];
        let pawns = pos.pieces[0][Piece::Pawn.index()] | pos.pieces[1][Piece::Pawn.index()];
        let ep = pos.ep.map(|s| s as u32).unwrap_or(0);
        let turn = pos.stm == Color::White;
        let rule50 = pos.halfmove as u32;

        let res = self.tb.probe_root(white, black, kings, queens, rooks, bishops, knights, pawns, rule50, ep, turn).ok()?;
        match res.root {
            pyrrhic_rs::DtzProbeValue::DtzResult(dtz_res) => {
                let mut p_clone = pos.clone();
                let legal = crate::movegen::generate_legal_list(&mut p_clone);
                for i in 0..legal.len() {
                    let mv = legal.get(i);
                    if mv.from == dtz_res.from_square && mv.to == dtz_res.to_square {
                        let promo = mv.flag.promo_piece();
                        let matches_promo = matches!(
                            (promo, dtz_res.promotion),
                            (None, PyrrhicPiece::Pawn)
                                | (Some(Piece::Queen), PyrrhicPiece::Queen)
                                | (Some(Piece::Rook), PyrrhicPiece::Rook)
                                | (Some(Piece::Bishop), PyrrhicPiece::Bishop)
                                | (Some(Piece::Knight), PyrrhicPiece::Knight)
                        );
                        if matches_promo {
                            return Some((mv, dtz_res.wdl));
                        }
                    }
                }
                None
            }
            _ => None,
        }
    }
}
