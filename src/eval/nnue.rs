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
    pub(crate) cvs: bool,
    is_core: bool,
    is_residual: bool,
    pub cvs_hidden: usize,
    /// Flat (inputs × hidden), row per feature.
    w1: Vec<f32>,
    b1: Vec<f32>,
    w2: Vec<f32>,
    cvs_w1: Vec<f32>,
    cvs_b1: Vec<f32>,
    cvs_w2: Vec<f32>,
    b2: f32,
    scale: f32,
    /// Optional output calibration (--nnue-cal): a monotone piecewise-linear map
    /// from the net's raw centipawn output to the oracle-labelled scale. Measured
    /// slope vs Stockfish's static eval was 0.49 (the net's range is ~2x
    /// compressed), which puts every centipawn margin in the search off by the
    /// same factor. Dense table over |raw| < CAL_MAX, sign restored on read, so
    /// eval symmetry is preserved exactly. None = identity (champion default).
    cal: Option<Vec<i32>>,
    /// Slope used beyond the table's last entry (linear tail extrapolation).
    cal_slope: f32,
    // ── Quantized inference (--quant-eval) ─────────────────────────────────────
    // i16 weights + i16 accumulator: 16 AVX2 lanes vs 8 for f32 on the hot
    // first layer. Quantized at load time from the f32 weights (S1=128).
    w1_q: Vec<i16>,
    b1_q: Vec<i16>,
    w2_q: Vec<i16>,
    b2_q: i32,
    /// Quantization scale (first layer). The cReLU clamp [0,1] becomes [0,QSCALE].
    qscale: i32,
    pub model_hash: u64,
    pub is_ranker: bool,
    pub ranker_w1: Vec<f32>,
    pub ranker_b1: Vec<f32>,
    pub ranker_w2: Vec<f32>,
    pub ranker_temperature: f32,
    pub ranker_max_bonus: i32,
}

// Keep each neuron's operations in the original order while allowing LLVM to
// vectorize across neurons. No SIMD-specific ISA or relaxed floating point.
#[inline]
fn copy_move_delta(dst: &mut [f32], src: &[f32], removed: &[f32], added: &[f32], captured: Option<&[f32]>) {
    assert_eq!(dst.len(), src.len());
    assert_eq!(dst.len(), removed.len());
    assert_eq!(dst.len(), added.len());
    if let Some(captured) = captured {
        assert_eq!(dst.len(), captured.len());
        for ((((out, value), from), victim), to) in dst.iter_mut().zip(src).zip(removed).zip(captured).zip(added) {
            *out = ((*value - *from) - *victim) + *to;
        }
    } else {
        for (((out, value), from), to) in dst.iter_mut().zip(src).zip(removed).zip(added) {
            *out = (*value - *from) + *to;
        }
    }
}

impl Nnue {
    pub fn load(path: &str, allow_unverified: bool) -> Result<Nnue, String> {
        let text = std::fs::read_to_string(path).map_err(|e| format!("read {path}: {e}"))?;
        let v: serde_json::Value =
            serde_json::from_str(&text).map_err(|e| format!("parse {path}: {e}"))?;
        let model_kind = v["modelKind"].as_str();
        let cvs = model_kind == Some("cvs_nnue");
        let is_residual = model_kind == Some("cvs_residual_nnue");
        let is_ranker = model_kind == Some("cvs_ranker_b");
        
        let hidden = if is_residual {
            v["psHidden"].as_u64().ok_or("missing psHidden")? as usize
        } else if is_ranker {
            0
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

        if is_ranker {
            let reg_hash = v["registryHash"].as_str().ok_or("missing registryHash")?;
            let want = format!("{:016x}", cvs_features::registry_hash());
            if reg_hash != want {
                return Err(format!("cvs_ranker_b registry hash mismatch: model {reg_hash} vs engine {want}"));
            }
            if v["anchorSchemaVersion"].as_u64().is_none() {
                return Err("missing anchorSchemaVersion".into());
            }
            if v["featureCount"].as_u64() != Some(168) {
                return Err("featureCount must be 168".into());
            }
            if v["sparseInputCount"].as_u64() != Some(504) {
                return Err("sparseInputCount must be 504".into());
            }
            if v["trainingCommit"].as_str().is_none() {
                return Err("missing trainingCommit".into());
            }
            if v["datasetManifestHash"].as_str().is_none() {
                return Err("missing datasetManifestHash".into());
            }
        }

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

        let w1 = if is_residual {
            parse_w1(&v, "ps_w1", expect_inputs, hidden)?
        } else if is_ranker {
            Vec::new()
        } else {
            parse_w1(&v, "w1", expect_inputs, hidden)?
        };
        let b1 = if is_residual {
            vecf(&v, "ps_b1")?
        } else if is_ranker {
            Vec::new()
        } else {
            vecf(&v, "b1")?
        };
        let w2 = if is_residual {
            vecf(&v, "ps_w2")?
        } else if is_ranker {
            Vec::new()
        } else {
            vecf(&v, "w2")?
        };
        
        let (cvs_w1, cvs_b1, cvs_w2) = if is_residual || is_ranker {
            let dim = if is_core { cvs_features::CVS_CORE_INPUT_DIM } else { cvs_features::CVS_INPUT_DIM };
            let dim = if is_ranker { 504 } else { dim };
            let cvs_hidden = if is_ranker {
                v["cvsHidden"].as_u64().ok_or("missing cvsHidden")? as usize
            } else {
                cvs_hidden
            };
            (
                parse_w1(&v, "cvs_w1", dim, cvs_hidden)?,
                vecf(&v, "cvs_b1")?,
                if is_residual { vecf(&v, "cvs_w2")? } else { Vec::<f32>::new() }
            )
        } else {
            (Vec::<f32>::new(), Vec::<f32>::new(), Vec::<f32>::new())
        };

        let (ranker_w1, ranker_b1, ranker_w2) = if is_ranker {
            (
                parse_w1(&v, "ranker_w1", 32, 64)?,
                vecf(&v, "ranker_b1")?,
                vecf(&v, "ranker_w2")?,
            )
        } else {
            (Vec::<f32>::new(), Vec::<f32>::new(), Vec::<f32>::new())
        };

        if !is_ranker && (b1.len() != hidden || w2.len() != hidden) {
            return Err("b1/w2 width mismatch".into());
        }
        let check_cvs_hidden = if is_ranker { 32 } else { cvs_hidden };
        if is_ranker && cvs_b1.len() != check_cvs_hidden {
            return Err("cvs_b1 width mismatch".into());
        }
        if is_residual && (cvs_b1.len() != cvs_hidden || cvs_w2.len() != cvs_hidden) {
            return Err("cvs_b1/cvs_w2 width mismatch".into());
        }
        
        let b2 = v["b2"].as_f64().ok_or("missing b2")? as f32;
        
        let mut model_hash = 0u64;
        if is_ranker {
            for &x in &ranker_w2 {
                model_hash = model_hash.wrapping_mul(31).wrapping_add(x.to_bits() as u64);
            }
            model_hash = model_hash.wrapping_mul(31).wrapping_add(scale.to_bits() as u64);
            model_hash = model_hash.wrapping_mul(31).wrapping_add(b2.to_bits() as u64);
        } else {
            for &x in &w2 {
                model_hash = model_hash.wrapping_mul(31).wrapping_add(x.to_bits() as u64);
            }
            for &x in &cvs_w2 {
                model_hash = model_hash.wrapping_mul(31).wrapping_add(x.to_bits() as u64);
            }
            model_hash = model_hash.wrapping_mul(31).wrapping_add(scale.to_bits() as u64);
            model_hash = model_hash.wrapping_mul(31).wrapping_add(b2.to_bits() as u64);
        }

        Ok(Nnue {
            hidden,
            inputs: expect_inputs,
            cvs: cvs || is_residual || is_ranker,
            is_core,
            is_residual,
            cvs_hidden: check_cvs_hidden,
            w1,
            b1,
            w2,
            cvs_w1,
            cvs_b1,
            cvs_w2,
            b2,
            scale,
            cal: None,
            cal_slope: 1.0,
            w1_q: Vec::new(),
            b1_q: Vec::new(),
            w2_q: Vec::new(),
            b2_q: 0,
            qscale: 128,
            model_hash,
            is_ranker,
            ranker_w1,
            ranker_b1,
            ranker_w2,
            ranker_temperature: if is_ranker { v["rankerTemperature"].as_f64().unwrap_or(1.0) as f32 } else { 1.0 },
            ranker_max_bonus: if is_ranker { v["rankerMaxBonus"].as_i64().unwrap_or(4000) as i32 } else { 4000 },
        })
    }

    pub fn registry_hash(&self) -> u64 {
        if self.is_core {
            cvs_features::core_registry_hash()
        } else {
            cvs_features::registry_hash()
        }
    }

    pub fn model_hash(&self) -> u64 {
        self.model_hash
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

    /// Copy a parent accumulator and apply a move in one pass per perspective.
    /// The subtraction/addition order matches `acc_apply` exactly; do not
    /// reassociate the deltas (floating-point rounding is search-visible).
    /// Call with the position BEFORE making the move and equally sized buffers.
    pub fn acc_apply_from(
        &self, child: &mut Accumulator, parent: &Accumulator, pos: &Position, mv: Move,
    ) {
        if matches!(mv.flag, MoveFlag::KingCastle | MoveFlag::QueenCastle) {
            child.white.copy_from_slice(&parent.white);
            child.black.copy_from_slice(&parent.black);
            self.acc_apply(child, pos, mv);
            return;
        }
        let us = pos.stm.index();
        let moving = pos.piece_at(mv.from).expect("acc_apply_from: empty from-square").1;
        let placed = mv.flag.promo_piece().unwrap_or(moving);
        let captured = if mv.flag == MoveFlag::EnPassant {
            Some((Piece::Pawn, mv.to ^ 8))
        } else if mv.flag.is_capture() {
            Some((pos.piece_at(mv.to).expect("acc_apply_from: empty capture-square").1, mv.to))
        } else {
            None
        };
        let row = |color: usize, piece: Piece, square: u8, flip: bool| {
            let (color, square) = if flip { (1 - color, square ^ 56) } else { (color, square) };
            let offset = ((color * 6 + piece.index()) * 64 + square as usize) * self.hidden;
            &self.w1[offset..offset + self.hidden]
        };
        for (dst, src, flip) in [
            (&mut child.white, &parent.white, false),
            (&mut child.black, &parent.black, true),
        ] {
            copy_move_delta(
                dst, src, row(us, moving, mv.from, flip), row(us, placed, mv.to, flip),
                captured.map(|(piece, square)| row(1 - us, piece, square, flip)),
            );
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
            self.finish(out)
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
            self.finish(out)
        } else {
            let mut out = self.b2;
            for j in 0..h {
                out += self.w2[j] * base[j].clamp(0.0, 1.0);
            }
            self.finish(out)
        }
    }

    /// Centipawns from the side to move's perspective.
    /// Dense-table bound for the calibration curve (|raw| >= this extrapolates).
    const CAL_MAX: usize = 4096;

    /// Install an output calibration curve fitted on oracle labels: `points` are
    /// (|raw cp|, |calibrated cp|) pairs, sorted, starting at (0, 0). The curve is
    /// applied to |raw| with the sign restored, so the eval stays exactly
    /// antisymmetric. Also folds the curve into `model_hash` so two nets that
    /// differ only by calibration are distinct artifacts.
    pub fn set_calibration(&mut self, points: &[(f64, f64)]) {
        if points.len() < 2 {
            return;
        }
        let interp = |x: f64| -> f64 {
            let pts = points;
            if x <= pts[0].0 {
                return pts[0].1;
            }
            for i in 1..pts.len() {
                let (x0, y0) = pts[i - 1];
                let (x1, y1) = pts[i];
                if x <= x1 {
                    let t = if x1 > x0 { (x - x0) / (x1 - x0) } else { 0.0 };
                    return y0 + t * (y1 - y0);
                }
            }
            let (x0, y0) = pts[pts.len() - 2];
            let (x1, y1) = pts[pts.len() - 1];
            let slope = if x1 > x0 { (y1 - y0) / (x1 - x0) } else { 1.0 };
            y1 + slope * (x - x1)
        };
        let mut table = vec![0i32; Self::CAL_MAX + 1];
        for (a, slot) in table.iter_mut().enumerate() {
            *slot = interp(a as f64).round() as i32;
        }
        let (x0, y0) = points[points.len() - 2];
        let (x1, y1) = points[points.len() - 1];
        self.cal_slope = if x1 > x0 { ((y1 - y0) / (x1 - x0)) as f32 } else { 1.0 };
        let mut h = self.model_hash;
        for v in &table {
            h = h.wrapping_mul(31).wrapping_add(*v as u64);
        }
        self.model_hash = h;
        self.cal = Some(table);
    }


    /// Quantize the f32 weights to i16 at load time (S1=128). Error measured at
    /// 5.6 cp MAE over 300 positions — negligible against the eval's own noise.
    pub fn quantize(&mut self) {
        const S1: i32 = 128;
        self.qscale = S1;
        self.w1_q = self.w1.iter().map(|&v| (v * S1 as f32).round() as i16).collect();
        self.b1_q = self.b1.iter().map(|&v| (v * S1 as f32).round() as i16).collect();
        self.w2_q = self.w2.iter().map(|&v| (v * S1 as f32).round() as i16).collect();
        self.b2_q = (self.b2 * (S1 * S1) as f32).round() as i32;
    }

    pub fn is_quantized(&self) -> bool {
        !self.w1_q.is_empty()
    }

    /// Quantized side-to-move eval: same math as `eval_stm` but i16 throughout the
    /// first layer and i32 for the output, so the hot loops use 16-wide AVX2 ops.
    /// Only the raw (non-cvs, non-residual) path is quantized.
    pub fn eval_stm_q(&self, pos: &Position) -> i32 {
        debug_assert!(self.hidden <= 512);
        debug_assert!(self.is_quantized());
        let h = self.hidden;
        let mut acc = [0i16; 512];
        acc[..h].copy_from_slice(&self.b1_q[..h]);
        let flip = pos.stm == Color::Black;
        let qs = self.qscale;
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
                    let row = &self.w1_q[(plane * 64 + s) * h..(plane * 64 + s) * h + h];
                    for j in 0..h {
                        acc[j] += row[j];
                    }
                }
            }
        }
        let mut out = self.b2_q;
        for j in 0..h {
            let activated = acc[j].clamp(0, qs as i16);
            out += activated as i32 * self.w2_q[j] as i32;
        }
        // Dequantize: out_q / (S1*S1) is the raw net output, then apply outputScaleCp.
        ((out as f32) / ((qs * qs) as f32) * self.scale).round() as i32
    }

    /// Net output -> evaluated centipawns, applying the optional calibration.
    #[inline]
    fn finish(&self, out: f32) -> i32 {
        let raw = (out * self.scale).round() as i32;
        let Some(table) = &self.cal else {
            return raw;
        };
        let a = raw.unsigned_abs() as usize;
        let v = if a < table.len() {
            table[a]
        } else {
            table[table.len() - 1] + ((a - (table.len() - 1)) as f32 * self.cal_slope).round() as i32
        };
        if raw < 0 {
            -v
        } else {
            v
        }
    }

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
        self.finish(out)
    }

    pub fn eval_ranker_raw(&self, sparse_buf: &[u32], dense_buf: &[f32; 32]) -> f32 {
        if !self.is_ranker {
            return 0.0;
        }

        // 1. Sparse hidden sum of embeddings + cvs_b1, clamped to [0.0, 1.0]
        let ch = self.cvs_hidden;
        let mut sparse_h = vec![0.0f32; ch];
        sparse_h.copy_from_slice(&self.cvs_b1);

        for &id in sparse_buf {
            let f = id as usize;
            let row = &self.cvs_w1[f * ch..(f + 1) * ch];
            for j in 0..ch {
                sparse_h[j] += row[j];
            }
        }
        for j in 0..ch {
            sparse_h[j] = sparse_h[j].clamp(0.0, 1.0);
        }

        // 2. Concatenate sparse_h (32) and dense_buf (32) into combined (64)
        let mut combined = vec![0.0f32; 64];
        combined[..32].copy_from_slice(&sparse_h);
        combined[32..64].copy_from_slice(dense_buf);

        // 3. Compute fc1: combined * ranker_w1 + ranker_b1, clamp(0, 1)
        let rh = 32; // rankerHidden
        let mut fc1_h = vec![0.0f32; rh];
        fc1_h.copy_from_slice(&self.ranker_b1);

        for r in 0..rh {
            let row = &self.ranker_w1[r * 64..(r + 1) * 64];
            let mut sum = 0.0f32;
            for c in 0..64 {
                sum += combined[c] * row[c];
            }
            fc1_h[r] += sum;
            fc1_h[r] = fc1_h[r].clamp(0.0, 1.0);
        }

        // 4. Compute fc2: fc1_h * ranker_w2 + b2
        let mut out = self.b2;
        for j in 0..rh {
            out += self.ranker_w2[j] * fc1_h[j];
        }
        out
    }
}
