//! Rung-3 feature pack — the export-parameter families the product already
//! shows users (board control, loose pieces, best safe capture, safe checks,
//! pawn islands), ported as white-POV signed eval features. These join the
//! Rung-2 vector + kingDanger as inputs to the Rung-3 MLP head; none of them
//! carries a hand weight — the trained net owns all of their influence.
//!
//! Cost note: best_see_capture and safe_checks are the expensive ones (SEE and
//! per-square attacker probes). They run only inside the net path, which is
//! inert unless net weights are loaded — promoted-baseline evals are unchanged.
use crate::attacks::{
    attackers_of, bishop_attacks, king_attacks, knight_attacks, pawn_attacks, queen_attacks,
    rook_attacks,
};
use crate::see::{see, SEE_VALUE};
use crate::{file_of, Color, Piece, Position};

#[derive(Clone, Copy, Debug, Default)]
pub struct Rung3Features {
    /// Squares attacked by white minus black, /64 (the export's board-control share).
    pub board_control: f64,
    /// Non-pawn, non-king pieces with zero friendly defenders: black − white
    /// (the side with fewer loose pieces is better off).
    pub loose_pieces: f64,
    /// Best SEE-winning capture available, in pawns: white's best − black's best.
    /// The export's bestSafeCapture — standing tactical pressure the static
    /// material count cannot see.
    pub best_see_capture: f64,
    /// Safe checking moves available (destination not defended): white − black.
    /// The classic initiative/king-danger predictor.
    pub safe_checks: f64,
    /// Pawn islands: black − white (fewer islands is structurally better).
    pub pawn_islands: f64,
}

#[inline]
fn pop_lsb(b: &mut u64) -> u8 {
    let s = b.trailing_zeros() as u8;
    *b &= *b - 1;
    s
}

/// Union of all squares a side attacks (through the real occupancy).
fn attacked_set(pos: &Position, color: Color) -> u64 {
    let c = color.index();
    let mut set = 0u64;
    let mut t = pos.pieces[c][Piece::Pawn.index()];
    while t != 0 {
        set |= pawn_attacks(color, pop_lsb(&mut t));
    }
    let mut t = pos.pieces[c][Piece::Knight.index()];
    while t != 0 {
        set |= knight_attacks(pop_lsb(&mut t));
    }
    let mut t = pos.pieces[c][Piece::Bishop.index()];
    while t != 0 {
        set |= bishop_attacks(pop_lsb(&mut t), pos.all);
    }
    let mut t = pos.pieces[c][Piece::Rook.index()];
    while t != 0 {
        set |= rook_attacks(pop_lsb(&mut t), pos.all);
    }
    let mut t = pos.pieces[c][Piece::Queen.index()];
    while t != 0 {
        set |= queen_attacks(pop_lsb(&mut t), pos.all);
    }
    let mut t = pos.pieces[c][Piece::King.index()];
    while t != 0 {
        set |= king_attacks(pop_lsb(&mut t));
    }
    set
}

/// Count of non-pawn, non-king pieces with no friendly defender.
fn loose_count(pos: &Position, color: Color) -> i32 {
    let c = color.index();
    let mut n = 0;
    for p in 1..5usize {
        // knight..queen
        let mut t = pos.pieces[c][p];
        while t != 0 {
            let sq = pop_lsb(&mut t);
            if attackers_of(&pos.pieces, sq, color, pos.all) == 0 {
                n += 1;
            }
        }
    }
    n
}

/// Best SEE value (in pawns, ≥0) among `color`'s capture targets. Probes each
/// enemy non-king piece attacked by `color` and SEEs the cheapest options.
fn best_see(pos: &Position, color: Color) -> f64 {
    let e = color.flip().index();
    let mut best = 0i32;
    for p in 0..5usize {
        let mut t = pos.pieces[e][p];
        while t != 0 {
            let to = pop_lsb(&mut t);
            let mut from_set = attackers_of(&pos.pieces, to, color, pos.all);
            while from_set != 0 {
                let from = pop_lsb(&mut from_set);
                let v = see(pos, from, to);
                if v > best {
                    best = v;
                }
            }
        }
    }
    best as f64 / 100.0
}

/// Checking moves to UNDEFENDED squares for `color` (knight/bishop/rook/queen).
fn safe_check_count(pos: &Position, color: Color) -> i32 {
    let c = color.index();
    let enemy = color.flip();
    let ksq = pos.king_sq(enemy);
    let own_occ = pos.occ[c];
    let mut n = 0;
    // Squares FROM which each piece type would check the enemy king:
    let check_from: [(usize, u64); 4] = [
        (Piece::Knight.index(), knight_attacks(ksq)),
        (Piece::Bishop.index(), bishop_attacks(ksq, pos.all)),
        (Piece::Rook.index(), rook_attacks(ksq, pos.all)),
        (Piece::Queen.index(), queen_attacks(ksq, pos.all)),
    ];
    for (pi, targets) in check_from {
        let mut t = pos.pieces[c][pi];
        while t != 0 {
            let from = pop_lsb(&mut t);
            let reach = match pi {
                x if x == Piece::Knight.index() => knight_attacks(from),
                x if x == Piece::Bishop.index() => bishop_attacks(from, pos.all),
                x if x == Piece::Rook.index() => rook_attacks(from, pos.all),
                _ => queen_attacks(from, pos.all),
            };
            let mut dest = reach & targets & !own_occ;
            while dest != 0 {
                let sq = pop_lsb(&mut dest);
                if attackers_of(&pos.pieces, sq, enemy, pos.all) == 0 {
                    n += 1;
                }
            }
        }
    }
    n
}

/// Pawn islands: groups of contiguous files containing at least one pawn.
fn islands(pos: &Position, color: Color) -> i32 {
    let pawns = pos.pieces[color.index()][Piece::Pawn.index()];
    let mut files = 0u8;
    let mut t = pawns;
    while t != 0 {
        files |= 1 << file_of(pop_lsb(&mut t));
    }
    let mut n = 0;
    let mut prev = false;
    for f in 0..8 {
        let has = files & (1 << f) != 0;
        if has && !prev {
            n += 1;
        }
        prev = has;
    }
    n
}

pub fn extract_rung3(pos: &Position) -> Rung3Features {
    let w_att = attacked_set(pos, Color::White);
    let b_att = attacked_set(pos, Color::Black);
    Rung3Features {
        board_control: (w_att.count_ones() as f64 - b_att.count_ones() as f64) / 64.0,
        loose_pieces: (loose_count(pos, Color::Black) - loose_count(pos, Color::White)) as f64,
        best_see_capture: best_see(pos, Color::White) - best_see(pos, Color::Black),
        safe_checks: (safe_check_count(pos, Color::White) - safe_check_count(pos, Color::Black))
            as f64,
        pawn_islands: (islands(pos, Color::Black) - islands(pos, Color::White)) as f64,
    }
}

/// The Rung-3 net input vector: Rung-2's 23 features (incl. kingDanger) + the
/// 5 families above, in a FIXED order shared with the TS trainer. Any change
/// here is a breaking weights-format change — bump the net JSON's `inputDim`.
pub const RUNG3_INPUT_DIM: usize = 28;
pub const RUNG3_FEATURE_KEYS: [&str; RUNG3_INPUT_DIM] = [
    "mobilityKnight",
    "mobilityBishop",
    "mobilityRook",
    "mobilityQueen",
    "kingShield",
    "kingZonePressure",
    "kingOpenFile",
    "passedPawnMg",
    "passedPawnEg",
    "connectedPassedPawn",
    "rookOpenFile",
    "rookSemiOpenFile",
    "rookSeventh",
    "doubledPawn",
    "isolatedPawn",
    "bishopPairMg",
    "bishopPairEg",
    "hangingPiece",
    "kingCentralExposure",
    "enemyQueenNearKing",
    "openCenterKingPenalty",
    "kingEscapeDeficit",
    "kingDanger",
    "boardControl",
    "loosePieces",
    "bestSeeCapture",
    "safeChecks",
    "pawnIslands",
];

pub fn feature_vector(pos: &Position) -> [f64; RUNG3_INPUT_DIM] {
    let f2 = super::rung2::extract_rung2(pos);
    let f3 = extract_rung3(pos);
    [
        f2.mobility_knight,
        f2.mobility_bishop,
        f2.mobility_rook,
        f2.mobility_queen,
        f2.king_shield,
        f2.king_zone_pressure,
        f2.king_open_file,
        f2.passed_pawn_mg,
        f2.passed_pawn_eg,
        f2.connected_passed_pawn,
        f2.rook_open_file,
        f2.rook_semi_open_file,
        f2.rook_seventh,
        f2.doubled_pawn,
        f2.isolated_pawn,
        f2.bishop_pair_mg,
        f2.bishop_pair_eg,
        f2.hanging_piece,
        f2.king_central_exposure,
        f2.enemy_queen_near_king,
        f2.open_center_king_penalty,
        f2.king_escape_deficit,
        f2.king_danger,
        f3.board_control,
        f3.loose_pieces,
        f3.best_see_capture,
        f3.safe_checks,
        f3.pawn_islands,
    ]
}

// Keep SEE_VALUE referenced so the import list stays honest if best_see changes.
const _: [i32; 6] = SEE_VALUE;
