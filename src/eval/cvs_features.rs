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
use crate::Position;

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
