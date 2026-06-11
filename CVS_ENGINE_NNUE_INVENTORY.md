# CVS Engine + NNUE Inventory (Phase 1)

Read-only audit of the Rust engine's eval/search/NNUE paths, answering the
architecture brief's Phase-1 questions. Source commit: `747dbe5`.

## Evaluator modes that currently exist

Two functional modes, selected per-`Searcher`:
1. **`rung2_scalar`** (default) — handcrafted material + tapered PST + bishop
   pair + tempo + 23 Rung-2 hazard scalars. Entry: `eval::evaluate()`.
2. **`raw_nnue`** (opt-in) — 768 piece-square net, replaces the static eval
   when a net is loaded. Entry: `Nnue::eval_stm()`.

There is no separate `classical` mode — "classical" and "rung2_scalar" are the
same function with rung2 weights zero vs trained. `extract_rung3` exists but
its MLP head was rejected and is not wired into eval.

## Which is default

`rung2_scalar`. `Searcher::new()` sets `nnue: None`; `Searcher::with_nnue()`
opts in. Binaries default to no net; `--nnue <json>` enables raw NNUE.

## Where each piece lives

| Concern | Location |
|---|---|
| Handcrafted scalar eval | `src/eval/mod.rs` — `evaluate()` (stm POV, l.159) → `evaluate_white()` → `evaluate_white_float_nonterminal()` (l.89) |
| Rung-2 feature extraction | `src/eval/rung2.rs` — `extract_rung2()` (l.132) → `Rung2Features` struct |
| Scalar weights store/load | `src/eval/weights.rs` — `ValueWeights` + `Rung2Weights` (serde camelCase); JSON via `--base`/`--rung2` |
| PST tables | `src/eval/pst.rs` (Michniewski, post-`pstScale`) |
| Raw NNUE | `src/eval/nnue.rs` — `Nnue::load()` + `eval_stm()` (l.70), f32 full recompute |
| Model files | `chess-vision-studio/arena/out/nnue-*.json` (app repo) |
| Benchmark/gates | cutechess-cli vs `f:\tools\cvs-baselines\*.exe`; PGN tally scripts; `RSI_LOOP_REPORT.md` |

## NNUE input representation (the raw baseline)

768 = 12 piece-planes × 64 squares, **side-to-move perspective** (black: mirror
vertically `sq^56` + colorswap). Sparse sum via accumulator. This is the brief's
`raw_nnue_gen1` — the control the CVS-geometry net must beat.

## How search calls static eval — THE INSERTION POINT

`src/search.rs` routes **every** static/leaf eval through two methods:
- `static_eval(&mut Position)` (l.483) — null-move / RFP / futility static value.
- `leaf_eval(&Position, no_legal, checked)` (l.490) — qsearch stand-pat + depth-0.

Both already branch `if let Some(n) = &self.nnue { return n.eval_stm(pos) }`
before falling back to `evaluate()`. **This is exactly where CVS-NNUE inserts:**
add a third branch (or make `nnue` an enum of {Raw, Cvs}) so a CVS-geometry net
plugs into the same two call sites with zero search-shape change. Search never
calls the eval directly elsewhere (the l.347 `evaluate()` is just the pre-loop
seed score).

## Geometry the engine ALREADY computes (scalar-collapsed)

`Rung2Features` already derives, then immediately scalarizes: mobility (N/B/R/Q),
king_shield, king_zone_pressure, king_open_file, passed/connected/doubled/
isolated pawns, rook open/semi-open/seventh, bishop_pair, hanging_piece,
king_central_exposure, enemy_queen_near_king, open_center_king_penalty,
king_escape_deficit, king_danger. **The brief's thesis in one line:** these are
computed as named relationships then collapsed to f64 contributions — the CVS
upgrade keeps them as facts and emits feature IDs instead of pre-weighting them.

## Current timings (measured this session, contended box)

- Classical/rung2 static eval: **426 knps** in full search.
- raw NNUE f32: **530 knps** (+24%; net is cheaper than rung2 extraction).
- rung2 extraction ≈ **30%** of classical eval time → it is the speed budget
  CVS-Fast must live within.
- Single-thread search ≈ 1.0M nps raw; SMP 5.69M nps @14T.
- No standing 10ms-budget microbench yet — **gap: add one** (brief Gate 2).

## Known bottlenecks

1. Rung-2 extraction recomputes attack sets / SEE / king zones per leaf (no
   caching) — the prime CVS-Fast optimization target.
2. NNUE is full-recompute (no incremental accumulator) — fine at current width,
   will matter if hidden grows or eval-per-node rises.
3. No pawn-structure hash; pawn facts recomputed every node.

## Safe CVS-NNUE insertion summary

Add `cvs_nnue` as a third eval mode behind the existing `static_eval`/`leaf_eval`
branch points. Gate it on a feature-registry-hash match at load (reject loudly
on mismatch). Keep `rung2_scalar` default and untouched. The 768 raw net stays
as the named control arm. No existing mode is removed.
