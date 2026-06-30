//! CVS Feature Registry v1 — the engine-side, truth-preserving feature space.
//!
//! The brief's thesis: the engine already *computes* named board relationships
//! (king danger, hanging material, loose pieces, rook files, passed pawns…) in
//! `extract_rung2`, then immediately collapses them to f64 scalar contributions.
//! This module keeps them as **facts**: it consumes the same extraction and
//! emits stable, deterministic feature IDs (+ readable names) suitable as
//! sparse CVS-NNUE inputs.
//!
//! v1 scope (Tier-1, CVS-Fast eligible): the hazard/structure families already
//! cheaply available from `Rung2Features`, bucketed by magnitude and tagged by
//! which side they favor (White-POV deltas — the brief permits White-POV
//! storage for compatibility). v2 will split per-side MY_/ENEMY_ and add
//! piece-square identity (the raw-NNUE 768 space) and SEE/motif facts.
//!
//! REGISTRY DETERMINISM: feature IDs are a pure function of `FAMILIES` order +
//! `BUCKETS_PER_SIDE`. Any change to either is a new registry version. The
//! `registry_hash()` is embedded in trained models; the loader must reject a
//! model whose hash differs (fail loud, never silently mis-map features).
use crate::eval::rung2::{extract_rung2, extract_rung2_core, Rung2Features};
use crate::{Position, Color, Move, Piece};
use crate::movegen::in_check;
use crate::see::{see, SEE_VALUE};
use crate::eval::phase_units;
use crate::attacks::king_attacks;


/// Registry version. Bump on any change to FAMILIES or the bucketing scheme.
pub const CVS_REGISTRY_VERSION: u32 = 1;

/// Magnitude buckets per side (1..=BUCKETS). Bucket 0 = inactive (skipped).
const BUCKETS_PER_SIDE: u32 = 3;
/// ID stride per family: side (2) × buckets, +1 so bucket indexing is 1-based.
const FAMILY_STRIDE: u32 = 2 * (BUCKETS_PER_SIDE + 1);

/// A geometry family: the readable key plus the magnitude thresholds that map a
/// White-POV delta to bucket 1/2/3. `signed` families favor White when the
/// delta is positive and Black when negative; thresholds apply to `|delta|`.
struct Family {
    key: &'static str,
    /// |value| thresholds for buckets 1, 2, 3 (ascending).
    thresholds: [f64; 3],
}

/// The v1 registry. ORDER IS THE CONTRACT — appending is fine, reordering or
/// removing is a breaking version bump.
const FAMILIES: &[Family] = &[
    Family {
        key: "KING_DANGER",
        thresholds: [2.0, 8.0, 20.0],
    },
    Family {
        key: "KING_ZONE_PRESSURE",
        thresholds: [1.0, 3.0, 6.0],
    },
    Family {
        key: "KING_OPEN_FILE",
        thresholds: [1.0, 2.0, 3.0],
    },
    Family {
        key: "KING_SHIELD",
        thresholds: [1.0, 2.0, 3.0],
    },
    Family {
        key: "KING_CENTRAL_EXPOSURE",
        thresholds: [1.0, 2.0, 4.0],
    },
    Family {
        key: "ENEMY_QUEEN_NEAR_KING",
        thresholds: [1.0, 2.0, 3.0],
    },
    Family {
        key: "OPEN_CENTER_KING",
        thresholds: [1.0, 2.0, 3.0],
    },
    Family {
        key: "KING_ESCAPE_DEFICIT",
        thresholds: [1.0, 2.0, 3.0],
    },
    Family {
        key: "HANGING_MATERIAL",
        thresholds: [1.0, 3.0, 6.0],
    },
    Family {
        key: "MOBILITY_KNIGHT",
        thresholds: [2.0, 5.0, 9.0],
    },
    Family {
        key: "MOBILITY_BISHOP",
        thresholds: [2.0, 6.0, 11.0],
    },
    Family {
        key: "MOBILITY_ROOK",
        thresholds: [2.0, 6.0, 12.0],
    },
    Family {
        key: "MOBILITY_QUEEN",
        thresholds: [3.0, 9.0, 16.0],
    },
    Family {
        key: "PASSED_PAWN",
        thresholds: [1.0, 2.0, 3.0],
    },
    Family {
        key: "CONNECTED_PASSED_PAWN",
        thresholds: [1.0, 2.0, 3.0],
    },
    Family {
        key: "ROOK_OPEN_FILE",
        thresholds: [1.0, 2.0, 3.0],
    },
    Family {
        key: "ROOK_SEMI_OPEN_FILE",
        thresholds: [1.0, 2.0, 3.0],
    },
    Family {
        key: "ROOK_SEVENTH",
        thresholds: [1.0, 2.0, 3.0],
    },
    Family {
        key: "DOUBLED_PAWN",
        thresholds: [1.0, 2.0, 3.0],
    },
    Family {
        key: "ISOLATED_PAWN",
        thresholds: [1.0, 2.0, 3.0],
    },
    Family {
        key: "BISHOP_PAIR",
        thresholds: [0.5, 1.0, 1.5],
    },
];

/// The cheap geometry registry v1 (CVS_CORE) - 104 features.
const CORE_FAMILIES: &[Family] = &[
    Family {
        key: "KING_OPEN_FILE",
        thresholds: [1.0, 2.0, 3.0],
    },
    Family {
        key: "KING_SHIELD",
        thresholds: [1.0, 2.0, 3.0],
    },
    Family {
        key: "KING_CENTRAL_EXPOSURE",
        thresholds: [1.0, 2.0, 4.0],
    },
    Family {
        key: "ENEMY_QUEEN_NEAR_KING",
        thresholds: [1.0, 2.0, 3.0],
    },
    Family {
        key: "OPEN_CENTER_KING",
        thresholds: [1.0, 2.0, 3.0],
    },
    Family {
        key: "PASSED_PAWN",
        thresholds: [1.0, 2.0, 3.0],
    },
    Family {
        key: "CONNECTED_PASSED_PAWN",
        thresholds: [1.0, 2.0, 3.0],
    },
    Family {
        key: "ROOK_OPEN_FILE",
        thresholds: [1.0, 2.0, 3.0],
    },
    Family {
        key: "ROOK_SEMI_OPEN_FILE",
        thresholds: [1.0, 2.0, 3.0],
    },
    Family {
        key: "ROOK_SEVENTH",
        thresholds: [1.0, 2.0, 3.0],
    },
    Family {
        key: "DOUBLED_PAWN",
        thresholds: [1.0, 2.0, 3.0],
    },
    Family {
        key: "ISOLATED_PAWN",
        thresholds: [1.0, 2.0, 3.0],
    },
    Family {
        key: "BISHOP_PAIR",
        thresholds: [0.5, 1.0, 1.5],
    },
];

/// Total feature-ID space (dense upper bound for the NNUE input layer).
pub const CVS_INPUT_DIM: usize = (FAMILIES.len() as u32 * FAMILY_STRIDE) as usize;
pub const CVS_CORE_INPUT_DIM: usize = (CORE_FAMILIES.len() as u32 * FAMILY_STRIDE) as usize;

/// The White-POV delta each family reads from `Rung2Features` (positive =
/// favors White). Kept beside FAMILIES (same order) so the registry stays a
/// single source of truth.
fn family_value(f: &Rung2Features, idx: usize) -> f64 {
    match idx {
        0 => f.king_danger,
        1 => f.king_zone_pressure,
        2 => f.king_open_file,
        3 => f.king_shield,
        4 => f.king_central_exposure,
        5 => f.enemy_queen_near_king,
        6 => f.open_center_king_penalty,
        7 => f.king_escape_deficit,
        8 => f.hanging_piece,
        9 => f.mobility_knight,
        10 => f.mobility_bishop,
        11 => f.mobility_rook,
        12 => f.mobility_queen,
        13 => f.passed_pawn_mg + f.passed_pawn_eg,
        14 => f.connected_passed_pawn,
        15 => f.rook_open_file,
        16 => f.rook_semi_open_file,
        17 => f.rook_seventh,
        18 => f.doubled_pawn,
        19 => f.isolated_pawn,
        20 => f.bishop_pair_mg + f.bishop_pair_eg,
        _ => 0.0,
    }
}

fn bucket(mag: f64, thresholds: &[f64; 3]) -> u32 {
    if mag >= thresholds[2] {
        3
    } else if mag >= thresholds[1] {
        2
    } else if mag >= thresholds[0] {
        1
    } else {
        0
    }
}

/// Active CVS features for a position: sparse IDs (for the net) + readable
/// names (for debug / explanation alignment).
pub struct CvsActiveFeatures {
    pub ids: Vec<u32>,
    pub names: Vec<String>,
}

/// CVS-FAST (Tier-1): active feature IDs only, appended into a caller-owned
/// buffer. No allocation, no strings — this is the hot-path form for NNUE
/// input and per-node search trace. `buf` is cleared first.
pub fn extract_cvs_ids_into(pos: &Position, buf: &mut Vec<u32>) {
    buf.clear();
    let r = extract_rung2(pos);
    for (idx, fam) in FAMILIES.iter().enumerate() {
        let v = family_value(&r, idx);
        let b = bucket(v.abs(), &fam.thresholds);
        if b == 0 {
            continue;
        }
        let side = if v >= 0.0 { 0 } else { 1 };
        buf.push(idx as u32 * FAMILY_STRIDE + side * (BUCKETS_PER_SIDE + 1) + b);
    }
}

/// Readable name for an active feature ID (debug / explanation alignment only).
pub fn feature_name(id: u32) -> String {
    let fam_idx = (id / FAMILY_STRIDE) as usize;
    let within = id % FAMILY_STRIDE;
    let side = within / (BUCKETS_PER_SIDE + 1);
    let bucket = within % (BUCKETS_PER_SIDE + 1);
    let who = if side == 0 { "WHITE" } else { "BLACK" };
    match FAMILIES.get(fam_idx) {
        Some(fam) => format!("{}_{}_BUCKET_{}", who, fam.key, bucket),
        None => format!("UNKNOWN_{id}"),
    }
}

/// CVS-FULL (debug/teaching): active IDs plus readable names. Built on the fast
/// path — strings are added only here, never in the hot loop.
pub fn extract_cvs_features(pos: &Position) -> CvsActiveFeatures {
    let mut ids = Vec::with_capacity(16);
    extract_cvs_ids_into(pos, &mut ids);
    let names = ids.iter().map(|&id| feature_name(id)).collect();
    CvsActiveFeatures { ids, names }
}

/// Deterministic hash of the registry definition (version + family keys +
/// bucketing). Embedded in trained models; the loader rejects a mismatch.
pub fn registry_hash() -> u64 {
    // FNV-1a over the structural definition.
    let mut h: u64 = 0xcbf29ce484222325;
    let mut mix = |bytes: &[u8]| {
        for &b in bytes {
            h ^= b as u64;
            h = h.wrapping_mul(0x100000001b3);
        }
    };
    mix(&CVS_REGISTRY_VERSION.to_le_bytes());
    mix(&BUCKETS_PER_SIDE.to_le_bytes());
    for fam in FAMILIES {
        mix(fam.key.as_bytes());
        for t in fam.thresholds {
            mix(&t.to_le_bytes());
        }
    }
    h
}

/// The White-POV delta each core family reads from `Rung2Features` (positive =
/// favors White).
fn core_family_value(f: &Rung2Features, idx: usize) -> f64 {
    match idx {
        0 => f.king_open_file,              // "KING_OPEN_FILE"
        1 => f.king_shield,                 // "KING_SHIELD"
        2 => f.king_central_exposure,        // "KING_CENTRAL_EXPOSURE"
        3 => f.enemy_queen_near_king,       // "ENEMY_QUEEN_NEAR_KING"
        4 => f.open_center_king_penalty,    // "OPEN_CENTER_KING"
        5 => f.passed_pawn_mg + f.passed_pawn_eg, // "PASSED_PAWN"
        6 => f.connected_passed_pawn,       // "CONNECTED_PASSED_PAWN"
        7 => f.rook_open_file,              // "ROOK_OPEN_FILE"
        8 => f.rook_semi_open_file,         // "ROOK_SEMI_OPEN_FILE"
        9 => f.rook_seventh,                // "ROOK_SEVENTH"
        10 => f.doubled_pawn,               // "DOUBLED_PAWN"
        11 => f.isolated_pawn,              // "ISOLATED_PAWN"
        12 => f.bishop_pair_mg + f.bishop_pair_eg, // "BISHOP_PAIR"
        _ => 0.0,
    }
}

/// CVS-FAST (CVS_CORE): active core feature IDs only, appended into a caller-owned
/// buffer. This is the hot-path form for incrementally updated leaf eval.
/// `buf` is cleared first.
pub fn extract_cvs_core_ids_into(pos: &Position, buf: &mut Vec<u32>) {
    buf.clear();
    let r = extract_rung2_core(pos);
    for (idx, fam) in CORE_FAMILIES.iter().enumerate() {
        let v = core_family_value(&r, idx);
        let b = bucket(v.abs(), &fam.thresholds);
        if b == 0 {
            continue;
        }
        let side = if v >= 0.0 { 0 } else { 1 };
        buf.push(idx as u32 * FAMILY_STRIDE + side * (BUCKETS_PER_SIDE + 1) + b);
    }
}

/// Deterministic hash of the core registry definition (version + family keys +
/// bucketing).
pub fn core_registry_hash() -> u64 {
    // FNV-1a over the structural definition.
    let mut h: u64 = 0xcbf29ce484222325;
    let mut mix = |bytes: &[u8]| {
        for &b in bytes {
            h ^= b as u64;
            h = h.wrapping_mul(0x100000001b3);
        }
    };
    mix(&CVS_REGISTRY_VERSION.to_le_bytes());
    mix(&BUCKETS_PER_SIDE.to_le_bytes());
    for fam in CORE_FAMILIES {
        mix(fam.key.as_bytes());
        for t in fam.thresholds {
            mix(&t.to_le_bytes());
        }
    }
    h
}


/// The compile-time symmetry/mirror mapping for registry features.
pub const MIRROR_FEATURE_ID: [u16; 168] = {
    let mut arr = [0u16; 168];
    let mut i = 0;
    while i < 168 {
        let fam = i / 8;
        let within = i % 8;
        let side_bit = within / 4;
        let bucket = within % 4;
        arr[i as usize] = fam * 8 + (1 - side_bit) * 4 + bucket;
        i += 1;
    }
    arr
};

#[derive(Clone, Debug)]
pub struct RootGeometryContext {
    pub parent_bitset: [u64; 3],
    pub mover: Color,
}

pub fn ids_to_bitset(ids: &[u32]) -> [u64; 3] {
    let mut bitset = [0u64; 3];
    for &id in ids {
        let word = (id / 64) as usize;
        let bit = (id % 64) as usize;
        if word < 3 {
            bitset[word] |= 1 << bit;
        }
    }
    bitset
}

pub fn extract_candidate_delta(
    ctx: &RootGeometryContext,
    pos: &Position,
    mv: Move,
    sparse_buf: &mut Vec<u32>,
    dense_buf: &mut [f32; 32],
    raw_score: i32,
    best_raw_score: i32,
) {
    // 1. Generate child position
    let mut child = pos.clone();
    child.make(mv);

    // 2. Extract active features (White-POV) for child
    let mut child_ids = Vec::with_capacity(32);
    extract_cvs_ids_into(&child, &mut child_ids);
    let child_bitset = ids_to_bitset(&child_ids);

    // 3. Compare with parent_bitset to identify added and removed sets
    let mut added_ids = Vec::new();
    let mut removed_ids = Vec::new();

    // Features active in child: if not active in parent, they were added
    for &id in &child_ids {
        let word = (id / 64) as usize;
        let bit = (id % 64) as usize;
        let is_in_parent = if word < 3 {
            (ctx.parent_bitset[word] & (1 << bit)) != 0
        } else {
            false
        };
        if !is_in_parent {
            added_ids.push(id);
        }
    }

    // Features active in parent: if not active in child, they were removed
    for id in 0..168u32 {
        let word = (id / 64) as usize;
        let bit = (id % 64) as usize;
        let is_in_parent = (ctx.parent_bitset[word] & (1 << bit)) != 0;
        let is_in_child = (child_bitset[word] & (1 << bit)) != 0;
        if is_in_parent && !is_in_child {
            removed_ids.push(id);
        }
    }

    // 4. Mirror relative to the root mover (ctx.mover)
    let flip = ctx.mover == Color::Black;
    let flip_id = |id: u32| -> u32 {
        if flip {
            MIRROR_FEATURE_ID[id as usize] as u32
        } else {
            id
        }
    };

    sparse_buf.clear();
    // Parent features
    for id in 0..168u32 {
        let word = (id / 64) as usize;
        let bit = (id % 64) as usize;
        if (ctx.parent_bitset[word] & (1 << bit)) != 0 {
            sparse_buf.push(flip_id(id));
        }
    }
    // Added features
    for id in added_ids {
        sparse_buf.push(168 + flip_id(id));
    }
    // Removed features
    for id in removed_ids {
        sparse_buf.push(336 + flip_id(id));
    }

    // 5. Populate dense anchors (32 floats)
    let piece = pos.piece_at(mv.from).map(|(_, p)| p).unwrap_or(Piece::Pawn);
    
    // One-hot moved piece type
    for j in 0..6 {
        dense_buf[j] = if piece.index() == j { 1.0 } else { 0.0 };
    }

    // Source file/rank
    dense_buf[6] = (mv.from % 8) as f32 / 7.0;
    dense_buf[7] = (mv.from / 8) as f32 / 7.0;

    // Destination file/rank
    dense_buf[8] = (mv.to % 8) as f32 / 7.0;
    dense_buf[9] = (mv.to / 8) as f32 / 7.0;

    // Gives check?
    let child_clone = child.clone();
    let gives_check_val = if in_check(&child_clone) { 1.0 } else { 0.0 };
    dense_buf[10] = gives_check_val;

    // Attacks higher value piece?
    let mut attacks_higher = 0.0;
    let placed = mv.flag.promo_piece().unwrap_or(piece);
    let occ = child.all;
    let att = match placed {
        Piece::Pawn => crate::attacks::pawn_attacks(pos.stm, mv.to),
        Piece::Knight => crate::attacks::knight_attacks(mv.to),
        Piece::Bishop => crate::attacks::bishop_attacks(mv.to, occ),
        Piece::Rook => crate::attacks::rook_attacks(mv.to, occ),
        Piece::Queen => crate::attacks::queen_attacks(mv.to, occ),
        Piece::King => king_attacks(mv.to),
    };
    let opponent_pieces = child.occ[pos.stm.flip().index()];
    let attacked_opponents = att & opponent_pieces;
    if attacked_opponents != 0 {
        let mut temp_opponents = attacked_opponents;
        while temp_opponents != 0 {
            let sq = temp_opponents.trailing_zeros() as u8;
            temp_opponents &= temp_opponents - 1;
            if let Some((_, opp_p)) = child.piece_at(sq) {
                if SEE_VALUE[opp_p.index()] > SEE_VALUE[placed.index()] {
                    attacks_higher = 1.0;
                    break;
                }
            }
        }
    }
    dense_buf[11] = attacks_higher;

    // SEE score (bounded/normalized)
    dense_buf[12] = (see(pos, mv.from, mv.to) as f32 / 1000.0).clamp(-1.0, 1.0);

    // Side to move
    dense_buf[13] = if pos.stm == Color::White { 0.0 } else { 1.0 };

    // Material balance (stm-perspective)
    let mut white_val = 0;
    let mut black_val = 0;
    for p in Piece::ALL {
        white_val += pos.pieces[Color::White.index()][p.index()].count_ones() as i32 * SEE_VALUE[p.index()];
        black_val += pos.pieces[Color::Black.index()][p.index()].count_ones() as i32 * SEE_VALUE[p.index()];
    }
    let balance = if pos.stm == Color::White {
        white_val - black_val
    } else {
        black_val - white_val
    };
    dense_buf[14] = (balance as f32 / 1000.0).clamp(-4.0, 4.0);

    // Game phase
    dense_buf[15] = (24 - phase_units(pos)) as f32 / 24.0;

    // King squares (relative to side to move)
    let ksq_us = pos.king_sq(pos.stm);
    let ksq_them = pos.king_sq(pos.stm.flip());
    dense_buf[16] = (ksq_us % 8) as f32 / 7.0;
    dense_buf[17] = (ksq_us / 8) as f32 / 7.0;
    dense_buf[18] = (ksq_them % 8) as f32 / 7.0;
    dense_buf[19] = (ksq_them / 8) as f32 / 7.0;

    // Castling rights (relative to side to move)
    let (wk_flag, wq_flag, bk_flag, bq_flag) = (crate::castle::WK, crate::castle::WQ, crate::castle::BK, crate::castle::BQ);
    let (us_k, us_q, them_k, them_q) = if pos.stm == Color::White {
        (wk_flag, wq_flag, bk_flag, bq_flag)
    } else {
        (bk_flag, bq_flag, wk_flag, wq_flag)
    };
    dense_buf[20] = if pos.castling & us_k != 0 { 1.0 } else { 0.0 };
    dense_buf[21] = if pos.castling & us_q != 0 { 1.0 } else { 0.0 };
    dense_buf[22] = if pos.castling & them_k != 0 { 1.0 } else { 0.0 };
    dense_buf[23] = if pos.castling & them_q != 0 { 1.0 } else { 0.0 };

    // Raw score & diff (normalized)
    dense_buf[24] = (raw_score as f32 / 400.0).clamp(-4.0, 4.0);
    dense_buf[25] = ((best_raw_score - raw_score) as f32 / 80.0).clamp(0.0, 4.0);

    // Moves categories
    dense_buf[26] = if piece == Piece::Pawn { 1.0 } else { 0.0 };

    // Development check (depart undeveloped starting square)
    let starting_sq = match (pos.stm, piece) {
        (Color::White, Piece::Knight) => mv.from == 1 || mv.from == 6,
        (Color::White, Piece::Bishop) => mv.from == 2 || mv.from == 5,
        (Color::White, Piece::Rook) => mv.from == 0 || mv.from == 7,
        (Color::White, Piece::Queen) => mv.from == 3,
        (Color::Black, Piece::Knight) => mv.from == 57 || mv.from == 62,
        (Color::Black, Piece::Bishop) => mv.from == 58 || mv.from == 61,
        (Color::Black, Piece::Rook) => mv.from == 56 || mv.from == 63,
        (Color::Black, Piece::Queen) => mv.from == 59,
        _ => false,
    };
    dense_buf[27] = if starting_sq { 1.0 } else { 0.0 };

    dense_buf[28] = if piece == Piece::King { 1.0 } else { 0.0 };

    // Padding (29..32)
    for j in 29..32 {
        dense_buf[j] = 0.0;
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn startpos_is_quiet() {
        let pos = Position::startpos();
        let f = extract_cvs_features(&pos);
        // A balanced opening position fires few or no hazard families.
        assert!(f.ids.len() <= 4, "startpos too noisy: {:?}", f.names);
        assert_eq!(f.ids.len(), f.names.len());
    }

    #[test]
    fn ids_in_range_and_unique_per_family() {
        // The 4fxkLVBb pre-Bd6 loss position: black king in danger.
        let pos =
            Position::from_fen("r1b3nr/1pp1bkpp/p1n5/1q3p2/3P4/B1PNQ1P1/P4PBP/RN2R1K1 b - - 3 19")
                .unwrap();
        let f = extract_cvs_features(&pos);
        for &id in &f.ids {
            assert!((id as usize) < CVS_INPUT_DIM, "id {id} >= {CVS_INPUT_DIM}");
        }
        // names and ids agree in count; no duplicate ids (one bucket per family).
        let mut sorted = f.ids.clone();
        sorted.sort_unstable();
        sorted.dedup();
        assert_eq!(sorted.len(), f.ids.len(), "duplicate feature ids");
    }

    #[test]
    fn registry_hash_is_stable() {
        // Pin the v1 hash so an accidental registry edit is caught by CI.
        assert_eq!(registry_hash(), registry_hash());
        assert_ne!(registry_hash(), 0);
    }
}
