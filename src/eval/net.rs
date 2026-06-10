//! Rung-3 value net — a tiny fixed MLP (input → hidden ReLU → 1) whose output
//! is a white-POV centipawn ADJUSTMENT added on top of the promoted base +
//! Rung-2 eval. ~200 parameters, ~30 multiplies: capacity for the interaction
//! terms (attackers × exposure × queen presence) that three linear fits could
//! not express, at negligible search cost.
//!
//! The net is inert unless weights are loaded (`--net net.json`), so the
//! promoted baseline is bit-for-bit unaffected. JSON is camelCase — the
//! kingDanger/serde lesson: snake_case fields would silently no-op.
use serde::Deserialize;

#[derive(Clone, Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ValueNet {
    pub input_dim: usize,
    pub hidden_dim: usize,
    /// hidden_dim rows × input_dim cols.
    pub w1: Vec<Vec<f64>>,
    pub b1: Vec<f64>,
    pub w2: Vec<f64>,
    pub b2: f64,
    /// Output scale in centipawns (the trainer bakes targets to cp/scale).
    pub output_scale_cp: f64,
}

impl ValueNet {
    /// Validate dimensions once at load — a malformed file must fail loudly,
    /// never silently mis-evaluate.
    pub fn validate(&self) -> Result<(), String> {
        if self.w1.len() != self.hidden_dim {
            return Err(format!(
                "w1 rows {} != hiddenDim {}",
                self.w1.len(),
                self.hidden_dim
            ));
        }
        for (i, row) in self.w1.iter().enumerate() {
            if row.len() != self.input_dim {
                return Err(format!(
                    "w1[{i}] len {} != inputDim {}",
                    row.len(),
                    self.input_dim
                ));
            }
        }
        if self.b1.len() != self.hidden_dim {
            return Err(format!(
                "b1 len {} != hiddenDim {}",
                self.b1.len(),
                self.hidden_dim
            ));
        }
        if self.w2.len() != self.hidden_dim {
            return Err(format!(
                "w2 len {} != hiddenDim {}",
                self.w2.len(),
                self.hidden_dim
            ));
        }
        Ok(())
    }

    /// White-POV centipawn adjustment for a feature vector.
    pub fn forward(&self, x: &[f64]) -> f64 {
        debug_assert_eq!(x.len(), self.input_dim);
        let mut out = self.b2;
        for h in 0..self.hidden_dim {
            let mut a = self.b1[h];
            let row = &self.w1[h];
            for i in 0..self.input_dim {
                a += row[i] * x[i];
            }
            if a > 0.0 {
                out += self.w2[h] * a; // ReLU
            }
        }
        out * self.output_scale_cp
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn tiny() -> ValueNet {
        ValueNet {
            input_dim: 2,
            hidden_dim: 2,
            w1: vec![vec![1.0, 0.0], vec![0.0, -1.0]],
            b1: vec![0.0, 0.0],
            w2: vec![1.0, 1.0],
            b2: 0.5,
            output_scale_cp: 100.0,
        }
    }

    #[test]
    fn forward_relu_and_scale() {
        let n = tiny();
        n.validate().unwrap();
        // h0 = relu(3) = 3, h1 = relu(-4) = 0 → (3 + 0.5) * 100
        assert_eq!(n.forward(&[3.0, 4.0]), 350.0);
    }

    #[test]
    fn validate_rejects_bad_dims() {
        let mut n = tiny();
        n.w2 = vec![1.0];
        assert!(n.validate().is_err());
    }
}
