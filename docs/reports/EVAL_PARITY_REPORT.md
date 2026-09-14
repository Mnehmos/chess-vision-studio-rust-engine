# Rust R2 — value-eval parity report (vs the legacy TS reference)

**Goal (R2):** port the CVS value system to Rust — base material/PST/bishop-pair/
tempo (tapered) + the 18 Rung-2 hazard features + trained-weight loading — and
prove parity against the TS `evaluateWhiteFloat` reference.

## Result: ✅ EXACT parity (max diff 0.000000cp), ~413k mixed evals/sec

### Parity

Fixtures: 628 unique FENs = curated battery (startpos, endgames, kiwipete, the d4
forensic position, castling/ep positions) + every position in the multipv training
dataset. Exported from the TS engine by `arena/export-eval-fixtures.ts` (reference/
fixture export only); consumed by `cargo run --release --bin eval_parity`.

| Weights | Max \|Rust − TS\| | Positions > 1cp |
|---|---:|---:|
| default (handcrafted) | **0.000000 cp** | 0 |
| trained mixed base + Rung-2 | **0.000000 cp** | 0 |

The Rust eval is float-exact against the TS reference on every fixture, under both
weight sets — including JS `Math.round` semantics (half toward +∞) for the rounded
centipawn eval, chess.js-compatible terminal handling (checkmate / stalemate /
insufficient material / 50-move), and the trained-weight JSON files loading
byte-for-byte (serde field names match the snapshots).

### Speed (release, single thread)

| Eval | Throughput |
|---|---:|
| default (base terms only) | ~771,000 evals/sec |
| mixed + all 18 Rung-2 features | ~413,000 evals/sec |

For scale: the legacy TS engine *searched* ~2,000 nodes/sec in hot tactical
positions. The Rust **full hazard eval alone** runs ~200× faster than the TS
engine could visit nodes.

### What was ported (semantics identical to TS)

- `eval/pst.rs` — Michniewski PSTs, visual-order tables + vertical mirror for
  Black; EG tables reuse MG for non-kings (exactly like the TS reference).
- `eval/mod.rs` — `evaluate_white_float` / `evaluate_white` / `evaluate` (stm POV):
  terminal short-circuits → tapered material+PST (phase = non-pawn material / 24)
  → bishop pair → tempo → optional Rung-2 contribution. King material fixed.
- `eval/rung2.rs` — all 18 hazard features: mobility N/B/R/Q (pseudo-legal),
  king shield / zone pressure / open-file exposure, passed pawns (mg/eg tapered,
  TS advancement formula, connected), rook open/semi-open/7th, doubled/isolated
  pawns, tapered bishop pair, hanging material (attacked-and-undefended, pawns).
- `eval/weights.rs` — `ValueWeights` + `Rung2Weights` with serde names matching
  `value-weights-mixed.json` / `rung2-weights-mixed.json`; defaults reproduce the
  handcrafted eval; all-zero Rung-2 is inert (fast-path, like TS).

### Tests

`cargo test`: **24/24** — 6 perft (movegen oracle) + 10 R1 (SEE parity cases +
x-ray battery + in_check/gives_check/attackers) + 8 R2 eval sanity cases
(mirroring the TS `value.test.ts` battery + Rung-2 invariants).

### Acceptance (per the R2 gate)

- Rust eval matches TS within 1cp on the curated suite → **exact (0.000000)** ✅
- Trained Rung-2 weight loading → ✅ (same JSON snapshots)
- cargo test passes, perft still passes → ✅
- No TS engine capability added (fixture export only) → ✅

### Next (R3)

Search: negamax/αβ + quiescence with forcing-quiet extensions + TT + move
ordering + PV + telemetry — then R4 gate parity/superiority vs the TS engine on
the same Stockfish-scored gates.
