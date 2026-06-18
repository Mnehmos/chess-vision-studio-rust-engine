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
    is_core: bool,
    is_residual: bool,
    cvs_hidden: usize,
    /// Flat (inputs × hidden), row per feature.
    w1: Vec<f32>,
    b1: Vec<f32>,
    w2: Vec<f32>,
    cvs_w1: Vec<f32>,
    cvs_b1: Vec<f32>,
    cvs_w2: Vec<f32>,
    b2: f32,
    scale: f32,
}

impl Nnue {
    pub fn load(path: &str, allow_unverified: bool) -> Result<Nnue, String> {
        let text = std::fs::read_to_string(path).map_err(|e| format!("read {path}: {e}"))?;
        let v: serde_json::Value =
            serde_json::from_str(&text).map_err(|e| format!("parse {path}: {e}"))?;
        let model_kind = v["modelKind"].as_str();
        let cvs = model_kind == Some("cvs_nnue");
        let is_residual = model_kind == Some("cvs_residual_nnue");
        let hidden = if is_residual {
            v["psHidden"].as_u64().ok_or("missing psHidden")? as usize
        } else {
            v["hidden"].as_u64().ok_or("missing hidden")? as usize
        };
        let scale = v["outputScaleCp"].as_f64().ok_or("missing outputScaleCp")? as f32;
        let mut expect_inputs = NNUE_INPUTS;
        let mut is_core = false;
        let mut cvs_hidden = 0;
        
        let vecf = |val: &serde_json::Value, key: &str| -> Result<Vec<f32>, String> {
            val[key]
                .as_array()
                .ok_or(format!("missing {key}"))?
                .iter()
                .map(|x| x.as_f64().map(|f| f as f32).ok_or(format!("{key} entry")))
                .collect()
        };

        if cvs || is_residual {
            let cvs_dim = v["cvsDim"].as_u64().unwrap_or(cvs_features::CVS_INPUT_DIM as u64) as usize;
            if cvs_dim == cvs_features::CVS_CORE_INPUT_DIM {
                is_core = true;
            } else if cvs_dim != cvs_features::CVS_INPUT_DIM {
                return Err(format!("unsupported cvsDim: {}", cvs_dim));
            }
            
            if !is_residual {
                expect_inputs = NNUE_INPUTS + cvs_dim;
            } else {
                expect_inputs = NNUE_INPUTS;
                cvs_hidden = v["cvsHidden"].as_u64().ok_or("missing cvsHidden")? as usize;
            }
            
            // Only the stm-relative geometry convention is valid: white-POV CVS
            // ids beside stm-mirrored piece squares made the net side-blind
            // (v1 post-mortem: -527 Elo). Refuse the broken convention.
            if v["cvsStmRelative"].as_bool() != Some(true) {
                return Err("cvs_nnue model lacks cvsStmRelative=true (v1 convention is broken) — refusing to load".into());
            }
            // Registry compatibility is non-negotiable: a silent mismatch would
            // mis-map every geometry feature. Fail loudly.
            let want = if is_core {
                format!("{:016x}", cvs_features::core_registry_hash())
            } else {
                format!("{:016x}", cvs_features::registry_hash())
            };
            match v["registryHash"].as_str() {
                Some(got) => {
                    if got != want {
                        return Err(format!(
                            "cvs_nnue registry hash mismatch: model {got} vs engine {want} — refusing to load"
                        ));
                    }
                }
                None => {
                    if allow_unverified {
                        eprintln!("WARNING: Loading unverified CVS geometry model lacking registryHash! Proceed at your own risk.");
                    } else {
                        return Err("cvs_nnue model lacks registryHash (verification failed) — refusing to load. Use --allow-unverified-net to bypass this safety check.".into());
                    }
                }
            }
        }
        
        let parse_w1 = |val: &serde_json::Value, key: &str, expect_rows: usize, h: usize| -> Result<Vec<f32>, String> {
            let w1_rows = val[key].as_array().ok_or(format!("missing {}", key))?;
            if w1_rows.len() != expect_rows {
                return Err(format!("{} rows {} != {}", key, w1_rows.len(), expect_rows));
            }
            let mut w1 = Vec::with_capacity(expect_rows * h);
            for row in w1_rows {
                let row = row.as_array().ok_or(format!("{} row not array", key))?;
                if row.len() != h {
                    return Err(format!("{} row width mismatch", key));
                }
                for x in row {
                    w1.push(x.as_f64().ok_or(format!("{} entry", key))? as f32);
                }
            }
            Ok(w1)
        };

        let w1 = if is_residual { parse_w1(&v, "ps_w1", expect_inputs, hidden)? } else { parse_w1(&v, "w1", expect_inputs, hidden)? };
        let b1 = if is_residual { vecf(&v, "ps_b1")? } else { vecf(&v, "b1")? };
        let w2 = if is_residual { vecf(&v, "ps_w2")? } else { vecf(&v, "w2")? };
        
        let (cvs_w1, cvs_b1, cvs_w2) = if is_residual {
            let dim = if is_core { cvs_features::CVS_CORE_INPUT_DIM } else { cvs_features::CVS_INPUT_DIM };
            (
                parse_w1(&v, "cvs_w1", dim, cvs_hidden)?,
                vecf(&v, "cvs_b1")?,
                vecf(&v, "cvs_w2")?
            )
        } else {
            (Vec::new(), Vec::new(), Vec::new())
        };

        if b1.len() != hidden || w2.len() != hidden {
            return Err("b1/w2 width mismatch".into());
        }
        if is_residual && (cvs_b1.len() != cvs_hidden || cvs_w2.len() != cvs_hidden) {
            return Err("cvs_b1/cvs_w2 width mismatch".into());
        }
        
        let b2 = v["b2"].as_f64().ok_or("missing b2")? as f32;
        Ok(Nnue {
            hidden,
            inputs: expect_inputs,
            cvs: cvs || is_residual,
            is_core,
            is_residual,
            cvs_hidden,
            w1,
            b1,
            w2,
            cvs_w1,
            cvs_b1,
            cvs_w2,
            b2,
            scale,
        })
    }

    /// Incremental updates apply to all models; for cvs_nnue models, piece-squares
    /// are maintained incrementally, and geometry features are merged on the fly.
    pub fn supports_incremental(&self) -> bool {
        true
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

    /// Centipawns from `stm`'s perspective using the maintained accumulator.
    /// If it is a cvs_nnue model, cheap/core/full features are extracted from `pos`
    /// and added on the fly to a stack copy of the piece-square accumulator.
    pub fn eval_acc(&self, pos: &Position, acc: &Accumulator, stm: Color) -> i32 {
        let h = self.hidden;
        let base = match stm {
            Color::White => &acc.white,
            Color::Black => &acc.black,
        };
        
        if self.is_residual {
            let mut ids: Vec<u32> = Vec::with_capacity(32);
            if self.is_core {
                cvs_features::extract_cvs_core_ids_into(pos, &mut ids);
            } else {
                cvs_features::extract_cvs_ids_into(pos, &mut ids);
            }
            
            let mut cvs_side = [0f32; 512];
            let ch = self.cvs_hidden;
            cvs_side[..ch].copy_from_slice(&self.cvs_b1);
            
            let flip = stm == Color::Black;
            for mut id in ids {
                if flip {
                    let fam = id / 8;
                    let within = id % 8;
                    let side_bit = within / 4;
                    let bucket = within % 4;
                    id = fam * 8 + (1 - side_bit) * 4 + bucket;
                }
                let f = id as usize;
                let row = &self.cvs_w1[f * ch..f * ch + ch];
                for j in 0..ch {
                    cvs_side[j] += row[j];
                }
            }
            
            let mut out = self.b2;
            for j in 0..h {
                out += self.w2[j] * base[j].clamp(0.0, 1.0);
            }
            for j in 0..ch {
                out += self.cvs_w2[j] * cvs_side[j].clamp(0.0, 1.0);
            }
            (out * self.scale).round() as i32
        } else if self.cvs {
            let mut ids: Vec<u32> = Vec::with_capacity(32);
            if self.is_core {
                cvs_features::extract_cvs_core_ids_into(pos, &mut ids);
            } else {
                cvs_features::extract_cvs_ids_into(pos, &mut ids);
            }
            let mut side = [0f32; 512];
            side[..h].copy_from_slice(&base[..h]);

            let flip = stm == Color::Black;
            for mut id in ids {
                if flip {
                    let fam = id / 8;
                    let within = id % 8;
                    let side_bit = within / 4;
                    let bucket = within % 4;
                    id = fam * 8 + (1 - side_bit) * 4 + bucket;
                }
                let f = NNUE_INPUTS + id as usize;
                debug_assert!(f < self.inputs);
                let row = &self.w1[f * h..f * h + h];
                for j in 0..h {
                    side[j] += row[j];
                }
            }
            let mut out = self.b2;
            for j in 0..h {
                out += self.w2[j] * side[j].clamp(0.0, 1.0);
            }
            (out * self.scale).round() as i32
        } else {
            let mut out = self.b2;
            for j in 0..h {
                out += self.w2[j] * base[j].clamp(0.0, 1.0);
            }
            (out * self.scale).round() as i32
        }
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
        let mut out = self.b2;
        for j in 0..h {
            out += self.w2[j] * acc[j].clamp(0.0, 1.0);
        }

        if self.is_residual {
            let mut ids: Vec<u32> = Vec::with_capacity(32);
            if self.is_core {
                cvs_features::extract_cvs_core_ids_into(pos, &mut ids);
            } else {
                cvs_features::extract_cvs_ids_into(pos, &mut ids);
            }
            
            let ch = self.cvs_hidden;
            let mut cvs_side = [0f32; 512];
            cvs_side[..ch].copy_from_slice(&self.cvs_b1);
            
            let flip = pos.stm == Color::Black;
            for mut id in ids {
                if flip {
                    let fam = id / 8;
                    let within = id % 8;
                    let side_bit = within / 4;
                    let bucket = within % 4;
                    id = fam * 8 + (1 - side_bit) * 4 + bucket;
                }
                let f = id as usize;
                let row = &self.cvs_w1[f * ch..f * ch + ch];
                for j in 0..ch {
                    cvs_side[j] += row[j];
                }
            }
            for j in 0..ch {
                out += self.cvs_w2[j] * cvs_side[j].clamp(0.0, 1.0);
            }
        } else if self.cvs {
            // Geometry features ride at +768 in the same accumulator,
            // STM-RELATIVE: the registry emits white-POV side bits, so flip
            // the side when black is to move (mirrors the piece-square half).
            let mut ids: Vec<u32> = Vec::with_capacity(32);
            if self.is_core {
                cvs_features::extract_cvs_core_ids_into(pos, &mut ids);
            } else {
                cvs_features::extract_cvs_ids_into(pos, &mut ids);
            }
            let flip = pos.stm == Color::Black;
            for mut id in ids {
                if flip {
                    let fam = id / 8;
                    let within = id % 8;
                    let side_bit = within / 4;
                    let bucket = within % 4;
                    id = fam * 8 + (1 - side_bit) * 4 + bucket;
                }
                let f = NNUE_INPUTS + id as usize;
                debug_assert!(f < self.inputs);
                let row = &self.w1[f * h..f * h + h];
                for j in 0..h {
                    acc[j] += row[j];
                }
            }
            // For flat cvs_nnue, out is recalculated since acc changed
            out = self.b2;
            for j in 0..h {
                out += self.w2[j] * acc[j].clamp(0.0, 1.0);
            }
        }
        (out * self.scale).round() as i32
    }
}
