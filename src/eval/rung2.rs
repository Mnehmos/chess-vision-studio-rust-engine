//! Rung-2 hazard features — exact port of the legacy TS `src/value/rung2.ts`.
//! Every feature is a White-POV SIGNED scalar (positive = good for White), so a
//! term is just weight·feature. Semantics mirror the TS reference precisely:
//! pseudo-legal mobility (empty + enemy squares, pins ignored), king shield/zone/
//! open-file, passed pawns (mg/eg tapered + connected), rook files/7th, doubled +
//! isolated pawns, tapered bishop pair, and hanging material (attacked-and-
//! undefended, in pawns). Parity target: the TS `extractRung2Features`.
use crate::attacks::{
    attackers_of, bishop_attacks, king_attacks, knight_attacks, queen_attacks, rook_attacks,
};
use crate::eval::{phase_units, MAX_PHASE};
use crate::see::SEE_VALUE;
use crate::{file_of, rank_of, Color, Piece, Position};

#[derive(Clone, Copy, Debug, Default)]
pub struct Rung2Features {
    pub mobility_knight: f64,
    pub mobility_bishop: f64,
    pub mobility_rook: f64,
    pub mobility_queen: f64,
    pub king_shield: f64,
    pub king_zone_pressure: f64,
    pub king_open_file: f64,
    pub passed_pawn_mg: f64,
    pub passed_pawn_eg: f64,
    pub connected_passed_pawn: f64,
    pub rook_open_file: f64,
    pub rook_semi_open_file: f64,
    pub rook_seventh: f64,
    pub doubled_pawn: f64,
    pub isolated_pawn: f64,
    pub bishop_pair_mg: f64,
    pub bishop_pair_eg: f64,
    pub hanging_piece: f64,
    // --- 2B King-Exposure Head (loop 2; all White-POV signed, + favors White) ---
    /// Enemy-queen-conditioned king displacement: rank distance from home rank
    /// (+1 if on a central file c–f), mg-tapered. black − white.
    pub king_central_exposure: f64,
    /// Queen proximity: max(0, 4 − Chebyshev(queen, enemy king)) per queen.
    /// (White queen near Black king) − (Black queen near White king).
    pub enemy_queen_near_king: f64,
    /// King on a central file with an EMPTY pawn shield while the enemy queen is
    /// on the board, mg-tapered. black − white.
    pub open_center_king_penalty: f64,
    /// Flight-square shortage with enemy queen on: max(0, 3 − safe king moves).
    /// black − white (the side with fewer escapes is worse off).
    pub king_escape_deficit: f64,
    /// Nonlinear king-danger index (2B v3): quadratic over weighted attack
    /// units (N/B=2, R=3, Q=5, + missing shield pawns) on the king zone, ≥2
    /// attackers required, mg-tapered, capped at 40. black-king − white-king.
    pub king_danger: f64,
}

#[inline]
fn pop_lsb(b: &mut u64) -> u8 {
    let s = b.trailing_zeros() as u8;
    *b &= *b - 1;
    s
}

/// A `color` pawn on `sq` is passed if no enemy pawn lies strictly ahead of it on
/// its own file or an adjacent file (TS `isPassed`).
fn is_passed(enemy_pawns: u64, sq: u8, color: Color) -> bool {
    let f = file_of(sq) as i32;
    let r = rank_of(sq) as i32;
    for ff in (f - 1).max(0)..=(f + 1).min(7) {
        let mut rr = if color == Color::White { r + 1 } else { r - 1 };
        while (0..8).contains(&rr) {
            if enemy_pawns & (1u64 << (rr * 8 + ff)) != 0 {
                return false;
            }
            rr += if color == Color::White { 1 } else { -1 };
        }
    }
    true
}

/// Friendly pawns on the king's file ±1, one/two ranks ahead of the king (TS `shieldPawns`).
fn shield_pawns(own_pawns: u64, ksq: u8, color: Color) -> i32 {
    let kf = file_of(ksq) as i32;
    let kr = rank_of(ksq) as i32;
    let mut count = 0;
    for df in -1..=1 {
        let ff = kf + df;
        if !(0..8).contains(&ff) {
            continue;
        }
        for step in 1..=2 {
            let rr = if color == Color::White {
                kr + step
            } else {
                kr - step
            };
            if !(0..8).contains(&rr) {
                continue;
            }
            if own_pawns & (1u64 << (rr * 8 + ff)) != 0 {
                count += 1;
            }
        }
    }
    count
}

/// Squares in the king's zone (king + 8 neighbours) attacked by `by` (TS `kingZoneAttacked`).
fn king_zone_attacked(pos: &Position, ksq: u8, by: Color) -> i32 {
    let mut zone = king_attacks(ksq) | (1u64 << ksq);
    let mut count = 0;
    while zone != 0 {
        let sq = pop_lsb(&mut zone);
        if attackers_of(&pos.pieces, sq, by, pos.all) != 0 {
            count += 1;
        }
    }
    count
}

/// Files on the king's file ±1 with no friendly pawn (TS `kingFileExposure`).
fn king_file_exposure(own_pawn_files: &[i32; 8], kf: u8) -> i32 {
    let kf = kf as i32;
    let mut exposure = 0;
    for df in -1..=1 {
        let ff = kf + df;
        if (0..8).contains(&ff) && own_pawn_files[ff as usize] == 0 {
            exposure += 1;
        }
    }
    exposure
}

/// Extract the Rung-2 features (White-POV signed) for a position.
pub fn extract_rung2(pos: &Position) -> Rung2Features {
    let units = phase_units(pos);
    let mg_w = units as f64 / MAX_PHASE as f64;
    let eg_w = 1.0 - mg_w;
    let mut f = Rung2Features::default();

    let w = Color::White.index();
    let b = Color::Black.index();
    let occ = pos.all;

    // --- Mobility (pseudo-legal: empty + enemy squares) + bishop counts ---
    for (ci, sign) in [(w, 1.0f64), (b, -1.0f64)] {
        let not_own = !pos.occ[ci];
        let mut kn = pos.pieces[ci][Piece::Knight.index()];
        while kn != 0 {
            let sq = pop_lsb(&mut kn);
            f.mobility_knight += sign * (knight_attacks(sq) & not_own).count_ones() as f64;
        }
        let mut bi = pos.pieces[ci][Piece::Bishop.index()];
        while bi != 0 {
            let sq = pop_lsb(&mut bi);
            f.mobility_bishop += sign * (bishop_attacks(sq, occ) & not_own).count_ones() as f64;
        }
        let mut ro = pos.pieces[ci][Piece::Rook.index()];
        while ro != 0 {
            let sq = pop_lsb(&mut ro);
            f.mobility_rook += sign * (rook_attacks(sq, occ) & not_own).count_ones() as f64;
        }
        let mut qu = pos.pieces[ci][Piece::Queen.index()];
        while qu != 0 {
            let sq = pop_lsb(&mut qu);
            f.mobility_queen += sign * (queen_attacks(sq, occ) & not_own).count_ones() as f64;
        }
    }

    // --- Bishop pair (tapered) ---
    let wb = pos.pieces[w][Piece::Bishop.index()].count_ones() as i32;
    let bb = pos.pieces[b][Piece::Bishop.index()].count_ones() as i32;
    let pair_ind = (if wb >= 2 { 1 } else { 0 }) - (if bb >= 2 { 1 } else { 0 });
    f.bishop_pair_mg = pair_ind as f64 * mg_w;
    f.bishop_pair_eg = pair_ind as f64 * eg_w;

    // --- Pawn structure ---
    let wp = pos.pieces[w][Piece::Pawn.index()];
    let bp = pos.pieces[b][Piece::Pawn.index()];
    let mut w_files = [0i32; 8];
    let mut b_files = [0i32; 8];
    {
        let mut t = wp;
        while t != 0 {
            w_files[file_of(pop_lsb(&mut t)) as usize] += 1;
        }
        let mut t = bp;
        while t != 0 {
            b_files[file_of(pop_lsb(&mut t)) as usize] += 1;
        }
    }
    let adj = |files: &[i32; 8], file: usize| -> i32 {
        let mut n = 0;
        if file > 0 {
            n += files[file - 1];
        }
        if file < 7 {
            n += files[file + 1];
        }
        n
    };
    let mut doubled_signed = 0i32;
    let mut isolated_signed = 0i32;
    for file in 0..8usize {
        if w_files[file] > 1 {
            doubled_signed -= w_files[file] - 1;
        }
        if b_files[file] > 1 {
            doubled_signed += b_files[file] - 1;
        }
        if w_files[file] > 0 && adj(&w_files, file) == 0 {
            isolated_signed -= w_files[file];
        }
        if b_files[file] > 0 && adj(&b_files, file) == 0 {
            isolated_signed += b_files[file];
        }
    }
    f.doubled_pawn = doubled_signed as f64;
    f.isolated_pawn = isolated_signed as f64;

    // Passed pawns. TS advancement: white pawn += (8 - gridRow - 1) = rank index;
    // black pawn -= (gridRow - 1) = (6 - rank index). Connected passer: any friendly
    // pawn on an adjacent file (any rank).
    let mut passed_signed = 0f64;
    let mut connected_signed = 0f64;
    {
        let mut t = wp;
        while t != 0 {
            let sq = pop_lsb(&mut t);
            if is_passed(bp, sq, Color::White) {
                passed_signed += rank_of(sq) as f64;
                if adj(&w_files, file_of(sq) as usize) > 0 {
                    connected_signed += 1.0;
                }
            }
        }
        let mut t = bp;
        while t != 0 {
            let sq = pop_lsb(&mut t);
            if is_passed(wp, sq, Color::Black) {
                passed_signed -= 6.0 - rank_of(sq) as f64;
                if adj(&b_files, file_of(sq) as usize) > 0 {
                    connected_signed -= 1.0;
                }
            }
        }
    }
    f.passed_pawn_mg = passed_signed * mg_w;
    f.passed_pawn_eg = passed_signed * eg_w;
    f.connected_passed_pawn = connected_signed;

    // --- Rook activity ---
    for (ci, sign) in [(w, 1.0f64), (b, -1.0f64)] {
        let mut ro = pos.pieces[ci][Piece::Rook.index()];
        while ro != 0 {
            let sq = pop_lsb(&mut ro);
            let file = file_of(sq) as usize;
            let (own_p, enemy_p) = if ci == w {
                (w_files[file], b_files[file])
            } else {
                (b_files[file], w_files[file])
            };
            if own_p == 0 && enemy_p == 0 {
                f.rook_open_file += sign;
            } else if own_p == 0 && enemy_p > 0 {
                f.rook_semi_open_file += sign;
            }
            // 7th rank from the rook's own side: White on rank index 6, Black on 1.
            let r = rank_of(sq);
            if (ci == w && r == 6) || (ci == b && r == 1) {
                f.rook_seventh += sign;
            }
        }
    }

    // --- King safety ---
    let wk = pos.king_sq(Color::White);
    let bk = pos.king_sq(Color::Black);
    f.king_shield =
        (shield_pawns(wp, wk, Color::White) - shield_pawns(bp, bk, Color::Black)) as f64;
    f.king_zone_pressure = (king_zone_attacked(pos, bk, Color::White)
        - king_zone_attacked(pos, wk, Color::Black)) as f64;
    f.king_open_file = (king_file_exposure(&b_files, file_of(bk))
        - king_file_exposure(&w_files, file_of(wk))) as f64;

    // --- 2B King-Exposure Head (enemy-queen-conditioned; the g14/g13/mini-g04
    // loss family: quiet-position king danger the base terms cannot see) ---
    {
        let wq = pos.pieces[w][Piece::Queen.index()];
        let bq = pos.pieces[b][Piece::Queen.index()];
        let cheb = |a: u8, c: u8| -> i32 {
            let df = (file_of(a) as i32 - file_of(c) as i32).abs();
            let dr = (rank_of(a) as i32 - rank_of(c) as i32).abs();
            df.max(dr)
        };
        // Queen proximity (both directions, independent of own-king exposure).
        let mut near = 0i32;
        let mut t = wq;
        while t != 0 {
            let q = pop_lsb(&mut t);
            near += (4 - cheb(q, bk)).max(0);
        }
        let mut t = bq;
        while t != 0 {
            let q = pop_lsb(&mut t);
            near -= (4 - cheb(q, wk)).max(0);
        }
        f.enemy_queen_near_king = near as f64;

        // Per-side exposure terms, conditioned on the ENEMY queen existing.
        let side_terms =
            |ksq: u8, own_pawns: u64, color: Color, enemy_queen: u64| -> (f64, f64, f64) {
                if enemy_queen == 0 {
                    return (0.0, 0.0, 0.0);
                }
                let home: i32 = if color == Color::White { 0 } else { 7 };
                let displacement = (rank_of(ksq) as i32 - home).abs();
                let central = (2..=5).contains(&file_of(ksq)); // files c–f
                let exposure = (displacement + if central { 1 } else { 0 }) as f64 * mg_w;
                let open_center = if central && shield_pawns(own_pawns, ksq, color) == 0 {
                    mg_w
                } else {
                    0.0
                };
                // Flight squares: king moves to squares not occupied by own pieces and
                // not attacked by the enemy.
                let mut flights = 0i32;
                let mut esc = king_attacks(ksq) & !pos.occ[color.index()];
                while esc != 0 {
                    let sq = pop_lsb(&mut esc);
                    if attackers_of(&pos.pieces, sq, color.flip(), pos.all) == 0 {
                        flights += 1;
                    }
                }
                let deficit = (3 - flights).max(0) as f64;
                (exposure, open_center, deficit)
            };
        let (we, wo, wd) = side_terms(wk, wp, Color::White, bq);
        let (be, bo, bd) = side_terms(bk, bp, Color::Black, wq);
        f.king_central_exposure = be - we;
        f.open_center_king_penalty = bo - wo;
        f.king_escape_deficit = bd - wd;
    }

    // --- Nonlinear king-danger index (2B v3). The 4 linear exposure scalars
    // above failed both training gates: too sparse to rank moves. Danger is
    // not additive — three attackers are ~9x one, not 3x — so this feature is
    // weighted attack units over the king zone fed through a quadratic, gated
    // on at least two attackers (one piece is a nuisance, not an attack).
    {
        let zone_danger = |ksq: u8, color: Color, own_pawns: u64| -> f64 {
            let zone = king_attacks(ksq) | (1u64 << ksq);
            let e = color.flip().index();
            let mut units = 0i32;
            let mut attackers = 0i32;
            let mut t = pos.pieces[e][Piece::Knight.index()];
            while t != 0 {
                let s = pop_lsb(&mut t);
                if knight_attacks(s) & zone != 0 {
                    units += 2;
                    attackers += 1;
                }
            }
            let mut t = pos.pieces[e][Piece::Bishop.index()];
            while t != 0 {
                let s = pop_lsb(&mut t);
                if bishop_attacks(s, pos.all) & zone != 0 {
                    units += 2;
                    attackers += 1;
                }
            }
            let mut t = pos.pieces[e][Piece::Rook.index()];
            while t != 0 {
                let s = pop_lsb(&mut t);
                if rook_attacks(s, pos.all) & zone != 0 {
                    units += 3;
                    attackers += 1;
                }
            }
            let mut t = pos.pieces[e][Piece::Queen.index()];
            while t != 0 {
                let s = pop_lsb(&mut t);
                if queen_attacks(s, pos.all) & zone != 0 {
                    units += 5;
                    attackers += 1;
                }
            }
            // Heavy-piece alignment: a rook/queen on the king's file presses
            // through blockers (the g14/g04 x-ray pattern direct attacks miss),
            // unless the attacker's own pawn closes the file.
            let kf = file_of(ksq);
            let file_mask = 0x0101010101010101u64 << kf;
            let enemy_pawns = pos.pieces[e][Piece::Pawn.index()];
            if enemy_pawns & file_mask == 0 {
                let mut heavy = (pos.pieces[e][Piece::Rook.index()]
                    | pos.pieces[e][Piece::Queen.index()])
                    & file_mask;
                while heavy != 0 {
                    let s = pop_lsb(&mut heavy);
                    // Direct zone attackers were already counted above.
                    if rook_attacks(s, pos.all) & zone == 0 {
                        units += 2;
                        attackers += 1;
                    }
                }
            }
            if attackers < 2 {
                return 0.0;
            }
            units += (3 - shield_pawns(own_pawns, ksq, color)).max(0);
            ((units * units) as f64 / 16.0).min(40.0)
        };
        f.king_danger =
            (zone_danger(bk, Color::Black, bp) - zone_danger(wk, Color::White, wp)) * mg_w;
    }

    // --- Hanging material (attacked-and-undefended non-king pieces, in pawns) ---
    let mut white_hanging = 0i32;
    let mut black_hanging = 0i32;
    for (ci, color) in [(w, Color::White), (b, Color::Black)] {
        for p in 0..5usize {
            // pawn..queen (kings excluded)
            let mut t = pos.pieces[ci][p];
            while t != 0 {
                let sq = pop_lsb(&mut t);
                let enemy = color.flip();
                if attackers_of(&pos.pieces, sq, enemy, occ) != 0
                    && attackers_of(&pos.pieces, sq, color, occ) == 0
                {
                    if ci == w {
                        white_hanging += SEE_VALUE[p];
                    } else {
                        black_hanging += SEE_VALUE[p];
                    }
                }
            }
        }
    }
    f.hanging_piece = (black_hanging - white_hanging) as f64 / 100.0;

    f
}

/// Extract only the cheap Rung-2 features (omitting expensive mobility/danger/hanging/safe-squares calculations).
pub fn extract_rung2_core(pos: &Position) -> Rung2Features {
    let units = phase_units(pos);
    let mg_w = units as f64 / MAX_PHASE as f64;
    let eg_w = 1.0 - mg_w;
    let mut f = Rung2Features::default();

    let w = Color::White.index();
    let b = Color::Black.index();

    // --- Bishop pair (tapered) ---
    let wb = pos.pieces[w][Piece::Bishop.index()].count_ones() as i32;
    let bb = pos.pieces[b][Piece::Bishop.index()].count_ones() as i32;
    let pair_ind = (if wb >= 2 { 1 } else { 0 }) - (if bb >= 2 { 1 } else { 0 });
    f.bishop_pair_mg = pair_ind as f64 * mg_w;
    f.bishop_pair_eg = pair_ind as f64 * eg_w;

    // --- Pawn structure ---
    let wp = pos.pieces[w][Piece::Pawn.index()];
    let bp = pos.pieces[b][Piece::Pawn.index()];
    let mut w_files = [0i32; 8];
    let mut b_files = [0i32; 8];
    {
        let mut t = wp;
        while t != 0 {
            w_files[file_of(pop_lsb(&mut t)) as usize] += 1;
        }
        let mut t = bp;
        while t != 0 {
            b_files[file_of(pop_lsb(&mut t)) as usize] += 1;
        }
    }
    let adj = |files: &[i32; 8], file: usize| -> i32 {
        let mut n = 0;
        if file > 0 {
            n += files[file - 1];
        }
        if file < 7 {
            n += files[file + 1];
        }
        n
    };
    let mut doubled_signed = 0i32;
    let mut isolated_signed = 0i32;
    for file in 0..8usize {
        if w_files[file] > 1 {
            doubled_signed -= w_files[file] - 1;
        }
        if b_files[file] > 1 {
            doubled_signed += b_files[file] - 1;
        }
        if w_files[file] > 0 && adj(&w_files, file) == 0 {
            isolated_signed -= w_files[file];
        }
        if b_files[file] > 0 && adj(&b_files, file) == 0 {
            isolated_signed += b_files[file];
        }
    }
    f.doubled_pawn = doubled_signed as f64;
    f.isolated_pawn = isolated_signed as f64;

    // Passed pawns
    let mut passed_signed = 0f64;
    let mut connected_signed = 0f64;
    {
        let mut t = wp;
        while t != 0 {
            let sq = pop_lsb(&mut t);
            if is_passed(bp, sq, Color::White) {
                passed_signed += rank_of(sq) as f64;
                if adj(&w_files, file_of(sq) as usize) > 0 {
                    connected_signed += 1.0;
                }
            }
        }
        let mut t = bp;
        while t != 0 {
            let sq = pop_lsb(&mut t);
            if is_passed(wp, sq, Color::Black) {
                passed_signed -= 6.0 - rank_of(sq) as f64;
                if adj(&b_files, file_of(sq) as usize) > 0 {
                    connected_signed -= 1.0;
                }
            }
        }
    }
    f.passed_pawn_mg = passed_signed * mg_w;
    f.passed_pawn_eg = passed_signed * eg_w;
    f.connected_passed_pawn = connected_signed;

    // --- Rook activity ---
    for (ci, sign) in [(w, 1.0f64), (b, -1.0f64)] {
        let mut ro = pos.pieces[ci][Piece::Rook.index()];
        while ro != 0 {
            let sq = pop_lsb(&mut ro);
            let file = file_of(sq) as usize;
            let (own_p, enemy_p) = if ci == w {
                (w_files[file], b_files[file])
            } else {
                (b_files[file], w_files[file])
            };
            if own_p == 0 && enemy_p == 0 {
                f.rook_open_file += sign;
            } else if own_p == 0 && enemy_p > 0 {
                f.rook_semi_open_file += sign;
            }
            let r = rank_of(sq);
            if (ci == w && r == 6) || (ci == b && r == 1) {
                f.rook_seventh += sign;
            }
        }
    }

    // --- King safety (cheap terms only) ---
    let wk = pos.king_sq(Color::White);
    let bk = pos.king_sq(Color::Black);
    f.king_shield =
        (shield_pawns(wp, wk, Color::White) - shield_pawns(bp, bk, Color::Black)) as f64;
    f.king_open_file = (king_file_exposure(&b_files, file_of(bk))
        - king_file_exposure(&w_files, file_of(wk))) as f64;

    // --- Queen proximity and home rank displacement ---
    {
        let wq = pos.pieces[w][Piece::Queen.index()];
        let bq = pos.pieces[b][Piece::Queen.index()];
        let cheb = |a: u8, c: u8| -> i32 {
            let df = (file_of(a) as i32 - file_of(c) as i32).abs();
            let dr = (rank_of(a) as i32 - rank_of(c) as i32).abs();
            df.max(dr)
        };
        let mut near = 0i32;
        let mut t = wq;
        while t != 0 {
            let q = pop_lsb(&mut t);
            near += (4 - cheb(q, bk)).max(0);
        }
        let mut t = bq;
        while t != 0 {
            let q = pop_lsb(&mut t);
            near -= (4 - cheb(q, wk)).max(0);
        }
        f.enemy_queen_near_king = near as f64;

        let side_terms =
            |ksq: u8, own_pawns: u64, color: Color, enemy_queen: u64| -> (f64, f64) {
                if enemy_queen == 0 {
                    return (0.0, 0.0);
                }
                let home: i32 = if color == Color::White { 0 } else { 7 };
                let displacement = (rank_of(ksq) as i32 - home).abs();
                let central = (2..=5).contains(&file_of(ksq));
                let exposure = (displacement + if central { 1 } else { 0 }) as f64 * mg_w;
                let open_center = if central && shield_pawns(own_pawns, ksq, color) == 0 {
                    mg_w
                } else {
                    0.0
                };
                (exposure, open_center)
            };
        let (we, wo) = side_terms(wk, wp, Color::White, bq);
        let (be, bo) = side_terms(bk, bp, Color::Black, wq);
        f.king_central_exposure = be - we;
        f.open_center_king_penalty = bo - wo;
    }

    f
}

/// White-POV centipawn contribution of the Rung-2 terms: Σ weight·feature.
/// Fast-path 0 when all weights are zero (matches the TS `rung2Contribution`).
pub fn rung2_contribution(pos: &Position, w: &super::weights::Rung2Weights) -> f64 {
    if w.is_zero() {
        return 0.0;
    }
    let f = extract_rung2(pos);
    w.mobility_knight * f.mobility_knight
        + w.mobility_bishop * f.mobility_bishop
        + w.mobility_rook * f.mobility_rook
        + w.mobility_queen * f.mobility_queen
        + w.king_shield * f.king_shield
        + w.king_zone_pressure * f.king_zone_pressure
        + w.king_open_file * f.king_open_file
        + w.passed_pawn_mg * f.passed_pawn_mg
        + w.passed_pawn_eg * f.passed_pawn_eg
        + w.connected_passed_pawn * f.connected_passed_pawn
        + w.rook_open_file * f.rook_open_file
        + w.rook_semi_open_file * f.rook_semi_open_file
        + w.rook_seventh * f.rook_seventh
        + w.doubled_pawn * f.doubled_pawn
        + w.isolated_pawn * f.isolated_pawn
        + w.bishop_pair_mg * f.bishop_pair_mg
        + w.bishop_pair_eg * f.bishop_pair_eg
        + w.hanging_piece * f.hanging_piece
        + w.king_central_exposure * f.king_central_exposure
        + w.enemy_queen_near_king * f.enemy_queen_near_king
        + w.open_center_king_penalty * f.open_center_king_penalty
        + w.king_escape_deficit * f.king_escape_deficit
        + w.king_danger * f.king_danger
}
