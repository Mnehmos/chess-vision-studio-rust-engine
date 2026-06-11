//! NNUE gen-1 inference — 768 sparse inputs (12 piece-planes × 64 squares,
//! side-to-move perspective) → HIDDEN clipped-ReLU → 1, centipawns from the
//! side to move. Float32 forward, full recompute per eval (no incremental
//! accumulator yet — the node-speed gate decides whether that is needed).
//!
//! Perspective rule (must match the trainer): white to move → features are
//! (piece, square) as-is; black to move → colors swapped and squares mirrored
//! vertically (sq ^ 56), so the net always sees "my pieces are planes 0–5".
use crate::eval::cvs_features;
use crate::{Color, Move, MoveFlag, Piece, Position};

pub const NNUE_INPUTS: usize = 768;
/// cvs_nnue input width: piece-square (768) + CVS registry v1 ids (168).
pub const CVS_NNUE_INPUTS: usize = NNUE_INPUTS + cvs_features::CVS_INPUT_DIM;

/// Both-perspective hidden-layer sums, maintained incrementally across
/// make/unmake by the searcher (a stack of these — clone-on-make, pop-on-
/// unmake). Two views because features are stm-relative: white's view uses
/// (color, square) as-is, black's swaps colors and mirrors squares.
#[derive(Clone)]
pub struct Accumulator {
    pub white: Vec<f32>,
    pub black: Vec<f32>,
}

#[derive(Clone)]
pub struct Nnue {
    hidden: usize,
    inputs: usize,
    /// True for cvs_nnue models (piece-square + CVS geometry ids).
    cvs: bool,
    /// Flat (inputs × hidden), row per feature.
    w1: Vec<f32>,
    b1: Vec<f32>,
    w2: Vec<f32>,
    b2: f32,
    scale: f32,
}

impl Nnue {
    pub fn load(path: &str) -> Result<Nnue, String> {
        let text = std::fs::read_to_string(path).map_err(|e| format!("read {path}: {e}"))?;
        let v: serde_json::Value =
            serde_json::from_str(&text).map_err(|e| format!("parse {path}: {e}"))?;
        let hidden = v["hidden"].as_u64().ok_or("missing hidden")? as usize;
        let scale = v["outputScaleCp"].as_f64().ok_or("missing outputScaleCp")? as f32;
        let cvs = v["modelKind"].as_str() == Some("cvs_nnue");
        if cvs {
            // Only the stm-relative geometry convention is valid: white-POV CVS
            // ids beside stm-mirrored piece squares made the net side-blind
            // (v1 post-mortem: -527 Elo). Refuse the broken convention.
            if v["cvsStmRelative"].as_bool() != Some(true) {
                return Err("cvs_nnue model lacks cvsStmRelative=true (v1 convention is broken) — refusing to load".into());
            }
            // Registry compatibility is non-negotiable: a silent mismatch would
            // mis-map every geometry feature. Fail loudly.
            let want = format!("{:016x}", cvs_features::registry_hash());
            let got = v["registryHash"].as_str().unwrap_or("(missing)");
            if got != want {
                return Err(format!(
                    "cvs_nnue registry hash mismatch: model {got} vs engine {want} — refusing to load"
                ));
            }
        }
        let expect_inputs = if cvs { CVS_NNUE_INPUTS } else { NNUE_INPUTS };
        let w1_rows = v["w1"].as_array().ok_or("missing w1")?;
        if w1_rows.len() != expect_inputs {
            return Err(format!("w1 rows {} != {expect_inputs}", w1_rows.len()));
        }
        let mut w1 = Vec::with_capacity(expect_inputs * hidden);
        for row in w1_rows {
            let row = row.as_array().ok_or("w1 row not array")?;
            if row.len() != hidden {
                return Err("w1 row width mismatch".into());
            }
            for x in row {
                w1.push(x.as_f64().ok_or("w1 entry")? as f32);
            }
        }
        let vecf = |key: &str| -> Result<Vec<f32>, String> {
            v[key]
                .as_array()
                .ok_or(format!("missing {key}"))?
                .iter()
                .map(|x| x.as_f64().map(|f| f as f32).ok_or(format!("{key} entry")))
                .collect()
        };
        let b1 = vecf("b1")?;
        let w2 = vecf("w2")?;
        if b1.len() != hidden || w2.len() != hidden {
            return Err("b1/w2 width mismatch".into());
        }
        let b2 = v["b2"].as_f64().ok_or("missing b2")? as f32;
        Ok(Nnue {
            hidden,
            inputs: expect_inputs,
            cvs,
            w1,
            b1,
            w2,
            b2,
            scale,
        })
    }

    /// Incremental updates only apply to pure piece-square models; cvs_nnue
    /// re-extracts geometry per position and stays on the full recompute.
    pub fn supports_incremental(&self) -> bool {
        !self.cvs
    }

    /// Both-perspective accumulator built from scratch (search-root entry).
    pub fn fresh_acc(&self, pos: &Position) -> Accumulator {
        let h = self.hidden;
        let mut acc = Accumulator {
            white: self.b1.clone(),
            black: self.b1.clone(),
        };
        debug_assert_eq!(acc.white.len(), h);
        for ci in 0..2usize {
            for p in Piece::ALL {
                let mut bb = pos.pieces[ci][p.index()];
                while bb != 0 {
                    let sq = bb.trailing_zeros() as usize;
                    bb &= bb - 1;
                    self.feat(&mut acc, ci, p, sq, 1.0);
                }
            }
        }
        acc
    }

    /// Add (+1) or remove (-1) one piece-square feature from BOTH perspective
    /// accumulators. White view: plane = color*6+piece, square as-is. Black
    /// view: colors swapped, square vertically mirrored — exactly the
    /// `eval_stm` perspective rule, maintained persistently.
    #[inline]
    fn feat(&self, acc: &mut Accumulator, ci: usize, p: Piece, sq: usize, sign: f32) {
        let h = self.hidden;
        let wf = (ci * 6 + p.index()) * 64 + sq;
        let bf = ((1 - ci) * 6 + p.index()) * 64 + (sq ^ 56);
        let wrow = &self.w1[wf * h..wf * h + h];
        let brow = &self.w1[bf * h..bf * h + h];
        for j in 0..h {
            acc.white[j] += sign * wrow[j];
            acc.black[j] += sign * brow[j];
        }
    }

    /// Apply `mv`'s feature deltas to `acc`. MUST be called with the position
    /// BEFORE `pos.make(mv)` — it reads the mover and capture target from the
    /// pre-move board. Mirrors Position::make's piece bookkeeping exactly.
    pub fn acc_apply(&self, acc: &mut Accumulator, pos: &Position, mv: Move) {
        let us = pos.stm;
        let them = us.flip();
        let (ui, ti) = (us.index(), them.index());
        let from = mv.from as usize;
        let to = mv.to as usize;
        let moving = pos
            .piece_at(mv.from)
            .map(|(_, p)| p)
            .expect("acc_apply: empty from-square");
        self.feat(acc, ui, moving, from, -1.0);
        match mv.flag {
            MoveFlag::EnPassant => {
                self.feat(acc, ti, Piece::Pawn, to ^ 8, -1.0);
                self.feat(acc, ui, Piece::Pawn, to, 1.0);
            }
            MoveFlag::KingCastle | MoveFlag::QueenCastle => {
                self.feat(acc, ui, Piece::King, to, 1.0);
                // Rook hop, same ranks as Position::make: king-side h->f,
                // queen-side a->d (relative to the king's destination).
                let (rf, rt) = if mv.flag == MoveFlag::KingCastle {
                    (to + 1, to - 1)
                } else {
                    (to - 2, to + 1)
                };
                self.feat(acc, ui, Piece::Rook, rf, -1.0);
                self.feat(acc, ui, Piece::Rook, rt, 1.0);
            }
            _ => {
                if mv.flag.is_capture() {
                    let cap = pos
                        .piece_at(mv.to)
                        .map(|(_, p)| p)
                        .expect("acc_apply: capture with empty to-square");
                    self.feat(acc, ti, cap, to, -1.0);
                }
                let placed = mv.flag.promo_piece().unwrap_or(moving);
                self.feat(acc, ui, placed, to, 1.0);
            }
        }
    }

    /// Centipawns from `stm`'s perspective using the maintained accumulator —
    /// the hot-path replacement for `eval_stm`'s full recompute.
    pub fn eval_acc(&self, acc: &Accumulator, stm: Color) -> i32 {
        let side = match stm {
            Color::White => &acc.white,
            Color::Black => &acc.black,
        };
        let mut out = self.b2;
        for j in 0..self.hidden {
            out += self.w2[j] * side[j].clamp(0.0, 1.0);
        }
        (out * self.scale).round() as i32
    }

    /// Centipawns from the side to move's perspective.
    pub fn eval_stm(&self, pos: &Position) -> i32 {
        debug_assert!(self.hidden <= 512);
        let mut acc = [0f32; 512];
        let h = self.hidden;
        acc[..h].copy_from_slice(&self.b1);
        let flip = pos.stm == Color::Black;
        for ci in 0..2usize {
            for p in Piece::ALL {
                let mut bb = pos.pieces[ci][p.index()];
                while bb != 0 {
                    let sq = bb.trailing_zeros() as usize;
                    bb &= bb - 1;
                    let (plane, s) = if flip {
                        ((1 - ci) * 6 + p.index(), sq ^ 56)
                    } else {
                        (ci * 6 + p.index(), sq)
                    };
                    let row = &self.w1[(plane * 64 + s) * h..(plane * 64 + s) * h + h];
                    for j in 0..h {
                        acc[j] += row[j];
                    }
                }
            }
        }
        if self.cvs {
            // Geometry features ride at +768 in the same accumulator,
            // STM-RELATIVE: the registry emits white-POV side bits, so flip
            // the side when black is to move (mirrors the piece-square half).
            let mut ids: Vec<u32> = Vec::with_capacity(32);
            cvs_features::extract_cvs_ids_into(pos, &mut ids);
            let flip = pos.stm == Color::Black;
            for mut id in ids {
                if flip {
                    let fam = id / 8;
                    let within = id % 8;
                    let side = within / 4;
                    let bucket = within % 4;
                    id = fam * 8 + (1 - side) * 4 + bucket;
                }
                let f = NNUE_INPUTS + id as usize;
                debug_assert!(f < self.inputs);
                let row = &self.w1[f * h..f * h + h];
                for j in 0..h {
                    acc[j] += row[j];
                }
            }
        }
        let mut out = self.b2;
        for j in 0..h {
            out += self.w2[j] * acc[j].clamp(0.0, 1.0);
        }
        (out * self.scale).round() as i32
    }
}
