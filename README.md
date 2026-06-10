# Chess Vision Studio Rust Engine

`cvs-bitboard-core` is the Rust chess engine used by Chess Vision Studio. It
provides the active search backend for the app, arena gauntlets, Lichess bot
experiments, and native Stockfish benchmark runs.

The engine is MIT licensed and intentionally small: bitboard move generation,
make/unmake, SEE, evaluation, alpha-beta search, UCI mode, and a JSON-line
analysis server live in this repository.

## Current Status

- Active engine path for Chess Vision Studio.
- Rust search is the promoted baseline over the legacy TypeScript search.
- Native Stockfish benchmarking is the official external strength anchor.
- WASM Stockfish gauntlets are retired as Elo anchors and kept only for cheap
  smoke tests or relative A/B signals.

As of the first native cutechess anchor on 2026-06-10, CVS scored:

| Native Stockfish UCI_Elo | Time control | Result | Score |
|---:|---|---|---:|
| 2000 | 10+0.1 | 14-1-5 | 72.5% |
| 2400 | 10+0.1 | 6-4-10 | 40.0% |
| 2800 | 10+0.1 | 0-1-19 | 2.5% |

The transferable claim is therefore roughly `2250-2350` in native Stockfish
UCI_Elo terms at fast blitz, not an official human rating.

## Binaries

```powershell
cargo build --release
```

This builds:

- `uci`: UCI frontend for cutechess, native Stockfish matches, and external
  tournament harnesses.
- `analyze`: JSON-line analysis server used by the Chess Vision Studio app and
  arena scripts.
- `perft`: perft runner and divide tool.
- `eval_parity`: evaluation parity checker against exported fixtures.
- `search_bench`: local search benchmark and telemetry runner.

## UCI Usage

Build the release binary, then point a UCI harness at:

```text
target/release/uci.exe
```

Optional weights:

```text
--base <value-weights.json> --rung2 <rung2-weights.json>
```

Example cutechess shape:

```powershell
cutechess-cli `
  -engine cmd=target/release/uci.exe name=CVS `
  -engine cmd=stockfish.exe name=Stockfish option.UCI_LimitStrength=true option.UCI_Elo=2400 `
  -each tc=10+0.1 `
  -games 80 `
  -repeat `
  -pgnout cvs-vs-sf2400.pgn
```

## Analyze Server

The app backend uses one long-lived `analyze --serve` process per depth.

```powershell
target/release/analyze.exe --serve --depth 6 --base ../chess-vision-studio/arena/out/value-weights-mixed.json --rung2 ../chess-vision-studio/arena/out/rung2-weights-mixed.json
```

Supported stdin commands:

- `<fen>`: search one FEN and emit one JSON result.
- `go <ms> <fen>`: search with a wall-clock budget.
- `eval <fen>`: static evaluation from White's point of view.
- `quit`: stop the server.

The response includes the selected UCI move, score, mate distance, PV, depth,
node counts, TT hits, cutoffs, quiescence telemetry, and elapsed time.

## Development Commands

```powershell
cargo fmt
cargo test
cargo test --release
cargo run --release --bin perft
cargo run --release --bin search_bench
```

Useful app-side checks live in the sibling `chess-vision-studio` repository:

```powershell
npx vitest run arena/__tests__/engine-backend.test.ts
npm run engine:compare -- --fen "<fen>" --depth 4 --backends ts,rust
```

## Search Stack

The promoted search includes:

- iterative deepening negamax with alpha-beta pruning
- capture quiescence with SEE filtering
- forcing quiet-check quiescence extensions
- MVV-LVA capture ordering
- transposition table with Zobrist hashing
- principal variation extraction
- repetition, insufficient-material, and fifty-move terminal handling
- optional danger-triggered root extension, gated off by default

Upcoming search work is tracked as isolated, separately gated patches:

1. killer and history move ordering
2. null-move pruning
3. late-move reductions

Each patch is judged against the previous promoted baseline through tests,
self-play SPRT, and the native SF-2400 cutechess gate.

## Evaluation Stack

The current promoted evaluation combines base material/PST-style terms with the
Rung-2 feature family. Rung-3 work is experimental until it passes the full gate.

Feature and weight experiments should not be promoted from regression metrics
alone. They need to pass:

- deterministic Rust tests
- evaluation parity checks where applicable
- held-out move quality gates
- forensic loss positions
- native cutechess promotion matches

## Reports

Important historical reports:

- `SEARCH_REPORT.md`: Rust search parity and speed versus the legacy TS engine.
- `R4_GATE_REPORT.md`: promotion gate that moved the app default to Rust.
- `R5_INTEGRATION_REPORT.md`: app/backend integration details.
- `GAUNTLET_REPORT.md`: Stockfish ladder history and native cutechess anchor.
- `RSI_LOOP_REPORT.md`: loss-family analysis and improvement priorities.
- `DANGER_EXTENSION_AB_REPORT.md`: gated danger-extension experiment.

## Claim Discipline

Benchmark claims should name the opponent, binary type, time control, game count,
weights, and harness. In particular:

- Native Stockfish via cutechess is the official external anchor.
- WASM Stockfish labels are not transferable Elo claims.
- Lichess bot games are useful real-world evidence, not controlled ratings.
- Official ratings fields such as FIDE or USCF should remain blank unless an
  actual federation rating exists.
