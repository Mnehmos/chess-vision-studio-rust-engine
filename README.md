# Chess Vision Studio Rust Engine

**The native CVS Engine for Chess Vision Studio.**

This repository contains the Rust chess engine that powers the local CVS Engine
panel, arena gauntlets, Lichess bot experiments, NNUE training loops, and native
Stockfish comparison gates.

It is intentionally local-first right now: clone it beside
`chess-vision-studio`, build it with Cargo, and the app's Vite dev server can
launch `analyze --serve` as a localhost-only engine bridge.

## Teaching Facts Protocol

`analyze --serve` accepts a distinct `{"cmd":"facts", ...}` JSON request for
deterministic teaching facts — this engine is the **truth layer** of Chess Vision
Studio's "Control Lens" teaching contract: it emits facts, never grades or prose.

The `TeachingFactBundleV1` protocol (facts registry **v5**) returns legal
played/best/refutation branches, each with full position facts: per-piece
attackers/defenders and SEE, named pawn-structure facts, king safety, available and
opponent-available motifs and pins, and deterministic position **hazards**
(losing-material, fork-threat, pin-constraint, king-pressure, mate-threat) with
move-to-move deltas. No topic classification or coaching prose — the app's teaching
compiler owns that. Validators live in `src/facts/`. See
[docs/TEACHING_FACTS_PROTOCOL.md](docs/TEACHING_FACTS_PROTOCOL.md).

## What It Provides

- Bitboard move generation and make/unmake.
- SEE and static evaluation.
- Iterative deepening alpha-beta search.
- UCI frontend for cutechess and external harnesses.
- JSON-line `analyze --serve` mode for the Chess Vision Studio app.
- Deterministic teaching-facts validators (`TeachingFactBundleV1`, registry v5):
  SEE, attackers/defenders, motifs/pins, pawn structure, king safety, hazards.
- Search telemetry for pruning, move ordering, TT, qsearch, and branching.
- NNUE and CVS feature experiments behind explicit gates.

## Engine Strength & Stack (2026-06-12)

Measured ~2525–2535 blitz vs native Stockfish rungs (bare gen7 eval ≈ 2375;
the full stack added ≈ +150 external Elo). Every layer below was validated
one-variable-at-a-time through the `benchmarks/` gate ladder — flag-off is
byte-identical to the prior champion, and nothing is called an "SPRT pass"
unless the bound was crossed.

**Champion stack:** gen8-v2 NNUE eval · incremental accumulator · futility ·
reverse futility (RFP) · TT-prune-store · qsearch-TT · history maluses +
history-informed LMR · ponder (opponent-clock).

| Layer | Gate result |
|---|---|
| gen7→gen8-v2 eval | **+115 Elo ±63, LOS 100%** (100-game head-to-head, net-swap only) |
| gen6→gen7 eval | formal SPRT +101.9 ±36.5 |
| ponder | formal SPRT +91 |
| RFP (depth≤4) | formal SPRT +68.8 ±39.6 |
| futility | accepted with note (fixed-N +34) |
| TT-prune-store | accepted with note (fixed-N +15.6; entry-found 22%→43%) |
| qsearch-TT | accepted with note (−7.4% nodes, 0 move changes) |
| history maluses + LMR | accepted with note (fixed-N 53.2%/400; −23% nodes, +3 depth) |

Selectivity doctrine: pruning validity is **conditional on eval calibration**
(futility was −188 Elo on classical eval, positive on gen7). Recorded
negatives kept behind flags: countermove, continuation-history, LMP, rule50
eval-scaling, king-activity, two-bucket TT — each rejected with a measured
reason in `RSI_LOOP_REPORT.md`. The TT diagnostic showed 82% of probe misses
are cold (under-filled, unique positions), so move-ordering — not table
tricks — is the remaining search lever.

NNUE training: raw 768→256, Stockfish-taught (`0.6·sigmoid(cp/256)+0.4·result`,
or pure-eval λ=1 for result-less corpora). gen8 lesson: the broad corpus must
match the engine's own position distribution; public data (Lichess evals,
Stockfish binpacks) supplements specific gaps (endgames) but cannot replace the
base. See `training/gen8/`.

## Local App Integration

Expected sibling layout:

```text
Github/
  chess-vision-studio/
  chess-vision-studio-rust-engine/
```

Build the engine:

```bash
cargo build --release
```

Then run the app:

```bash
cd ../chess-vision-studio
npm install
npm run dev
```

The app defaults to:

```text
../chess-vision-studio-rust-engine/target/release/analyze.exe
```

On macOS/Linux use:

```text
../chess-vision-studio-rust-engine/target/release/analyze
```

Configure the bridge in the app repo's `.env`:

```text
CVS_RUST_EXE=../chess-vision-studio-rust-engine/target/release/analyze.exe
CVS_RUST_DEPTH=6
CVS_RUST_BASE=arena/out/value-weights-mixed.json
CVS_RUST_RUNG2=arena/out/rung2-weights-mixed.json
CVS_RUST_FUTILITY=1
CVS_RUST_RFP=0
```

The app labels this native path as **CVS Engine**. Move grading and dataset
analysis are powered by **native Stockfish** (a pooled UCI subprocess, labeled
**Stockfish · native** in the app; WASM is an automatic fallback when no binary is
present).

## Binaries

```bash
cargo build --release
```

Release binaries:

- `target/release/uci`: UCI engine for cutechess, native Stockfish matches, and
  tournament harnesses.
- `target/release/analyze`: JSON-line analysis server used by the app and arena
  scripts.
- `target/release/perft`: perft runner and divide tool.
- `target/release/eval_parity`: evaluation parity checker.
- `target/release/search_bench`: local search benchmark and telemetry runner.

Windows builds use `.exe` suffixes.

## Analyze Server

Run a long-lived local analysis process:

```bash
target/release/analyze --serve --depth 6 \
  --base ../chess-vision-studio/arena/out/value-weights-mixed.json \
  --rung2 ../chess-vision-studio/arena/out/rung2-weights-mixed.json \
  --futility
```

Supported stdin commands:

```text
<fen>             search one FEN
go <ms> <fen>    search one FEN with a wall-clock budget
eval <fen>       static eval from White's point of view
quit             stop the server
```

The response is one JSON line with best move, score, mate distance, PV, depth,
nodes, qnodes, TT hits, elapsed time, and telemetry.

## UCI Usage

Point a UCI harness at:

```text
target/release/uci
```

Optional weights:

```text
--base <value-weights.json> --rung2 <rung2-weights.json>
```

Example cutechess shape:

```bash
cutechess-cli \
  -engine cmd=target/release/uci name=CVS \
  -engine cmd=stockfish name=Stockfish option.UCI_LimitStrength=true option.UCI_Elo=2400 \
  -each tc=10+0.1 \
  -games 80 \
  -repeat \
  -pgnout cvs-vs-sf2400.pgn
```

## Development

```bash
cargo fmt
cargo test
cargo test --release
cargo run --release --bin perft
cargo run --release --bin search_bench
```

Useful app-side checks in the sibling repository:

```bash
npx vitest run arena/__tests__/engine-backend.test.ts
npm run engine:compare -- --fen "<fen>" --depth 4 --backends ts,rust
```

## Current Anchor

The first native cutechess anchor on 2026-06-10 scored:

| Native Stockfish UCI_Elo | Time control | Result | Score |
|---:|---|---|---:|
| 2000 | 10+0.1 | 14-1-5 | 72.5% |
| 2400 | 10+0.1 | 6-4-10 | 40.0% |
| 2800 | 10+0.1 | 0-1-19 | 2.5% |

Treat this as a controlled engineering anchor, not a human rating claim.

## Search And Training Notes

Promoted and experimental work is tracked in:

- `SEARCH_REPORT.md`
- `SEARCH_PATCHES.md`
- `GAUNTLET_REPORT.md`
- `RSI_LOOP_REPORT.md`
- `GEN8_TRAINING_PLAN.md`
- `benchmarks/README.md`

Current Gen8 discipline:

- Keep the hot path raw, incremental, and speed-preserving.
- Use CVS geometry as side intelligence until it earns a per-node cost.
- Compare every candidate against the frozen Gen7 snapshot.
- Split train/validation/test by game/source, not random positions.
- Keep final evaluation slices locked out of training.

## Claim Discipline

Benchmark claims must name opponent, binary type, time control, game count,
weights, harness, and enabled search flags.

- Native Stockfish via cutechess is the official external anchor.
- Browser/WASM Stockfish labels are not transferable Elo claims.
- Lichess bot games are useful real-world evidence, not controlled ratings.
- Official rating fields such as FIDE or USCF should stay blank unless an actual
  federation rating exists.

## License

[MIT](LICENSE)
