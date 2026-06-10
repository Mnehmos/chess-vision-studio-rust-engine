//! NNUE gen-1 inference — 768 sparse inputs (12 piece-planes × 64 squares,
//! side-to-move perspective) → HIDDEN clipped-ReLU → 1, centipawns from the
//! side to move. Float32 forward, full recompute per eval (no incremental
//! accumulator yet — the node-speed gate decides whether that is needed).
//!
//! Perspective rule (must match the trainer): white to move → features are
//! (piece, square) as-is; black to move → colors swapped and squares mirrored
//! vertically (sq ^ 56), so the net always sees "my pieces are planes 0–5".
use crate::{Color, Piece, Position};

pub const NNUE_INPUTS: usize = 768;

#[derive(Clone)]
pub struct Nnue {
    hidden: usize,
    /// Flat (768 × hidden), row per feature.
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
        let w1_rows = v["w1"].as_array().ok_or("missing w1")?;
        if w1_rows.len() != NNUE_INPUTS {
            return Err(format!("w1 rows {} != {NNUE_INPUTS}", w1_rows.len()));
        }
        let mut w1 = Vec::with_capacity(NNUE_INPUTS * hidden);
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
            w1,
            b1,
            w2,
            b2,
            scale,
        })
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
        (out * self.scale).round() as i32
    }
}
