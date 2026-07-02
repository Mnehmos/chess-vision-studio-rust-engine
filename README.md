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

The `TeachingFactBundleV1` protocol (facts registry **v22**) returns legal
played/best/refutation branches, each with full position facts: per-piece
attackers/defenders and SEE, named pawn-structure facts, king safety, 64-square
control, deterministic position **hazards** (losing-material, fork-threat,
pin-constraint, king-pressure, mate-threat) with move-to-move deltas, and
**18 validated motif detectors** for both sides — fork, pin, skewer, discovery
(incl. discovered/double/discoverer checks), discovered defense, capturing /
attacking / deflecting / luring the defender, overload, interference, trapped
piece, desperado, double attack, x-ray attack/defense, win-the-exchange, and named
mate patterns. Every detector is adversarially fuzz-verified to zero false
positives (see [docs/DETECTOR_SOUNDNESS.md](docs/DETECTOR_SOUNDNESS.md)); the
motif-to-detector map lives in `benchmarks/data/motif-taxonomy.json` (44 of 197
taxonomy motifs detected). No topic classification or coaching prose — the app's
teaching compiler owns that. Validators live in `src/facts/`. See
[docs/TEACHING_FACTS_PROTOCOL.md](docs/TEACHING_FACTS_PROTOCOL.md).

## What It Provides

- Bitboard move generation and make/unmake.
- SEE and static evaluation.
- Iterative deepening alpha-beta search.
- UCI frontend for cutechess and external harnesses.
- JSON-line `analyze --serve` mode for the Chess Vision Studio app.
- Deterministic teaching-facts validators (`TeachingFactBundleV1`, registry v22):
  SEE, attackers/defenders, 18 motif detectors, pawn structure, king safety,
  square control, hazards.
- A deterministic fixed-node diagnostic interface (`nodeBudget` +
  `diagnosticIsolation` cold/warm on serve requests, `go nodes N` over UCI) for
  reproducible experiments.
- Search telemetry for pruning, move ordering, TT, qsearch, and branching.
- NNUE and CVS feature experiments behind explicit gates
  (see `CLASSICAL_EVAL_EXPERIMENT.md` for the standing eval-experiment program).

## Engine-development benchmarks (2026-06-12)

> **Current champion (2026-07-01):** the frozen baseline is **N0 =
> `g9.current-default.raw-plus-residual`** (gen9 raw NNUE + core104 residual
> helper + rung2), pinned with artifact SHAs, search profile, and live-bot flags
> in [`benchmarks/N0-identity.json`](benchmarks/N0-identity.json). The section
> below is the dated gen8-era gate history and is kept as a record.

These are **controlled engineering benchmarks against fixed native-Stockfish
settings — not human, FIDE, or otherwise transferable ratings.** Against pinned
Stockfish rungs the champion stack scores in the **~2525–2535 blitz band** (bare
gen7 eval ≈ 2375; the full stack added ≈ +150 Elo over the prior champion in
controlled self-play gates). Every layer below was validated one-variable-at-a-time
through the `benchmarks/` gate ladder — flag-off is byte-identical to the prior
champion, and nothing is called an "SPRT pass" unless the bound was crossed. See
**Claim Discipline** at the bottom; the band is an engineering anchor for tracking
progress, not a rating to advertise.

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
CVS_RUST_NNUE=arena/out/gen8-raw-h256-v2.json
CVS_RUST_THREADS=1
CVS_RUST_CVS_HELPERS=0
CVS_RUST_FUTILITY=1
CVS_RUST_RFP=1
CVS_RUST_TTPS=1
CVS_RUST_QTT=1
CVS_RUST_HISTMALUS=1
CVS_RUST_HISTLMR=1
```

The app labels this native path as **CVS Engine**. Move grading and dataset
analysis are powered by **native Stockfish** (a pooled UCI subprocess, labeled
**Stockfish · native** in the app; WASM is an automatic fallback when no binary is
present).

Keep `CVS_RUST_THREADS=1` and `CVS_RUST_CVS_HELPERS=0` for normal app/dataset
analysis unless you are running an explicit same-budget SMP or specialist-lane
benchmark. The app bridge scales its process pool down when per-process search
threads are enabled, but multi-process fan-out and multi-threaded search still
share the same CPU budget.

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

JSON requests carry game history and options:

```text
{"cmd":"go","budgetMs":500,"fen":"...","initialFen":"...","moves":[...]}
{"cmd":"eval","fen":"..."}
{"cmd":"facts","schemaVersion":1,"fenBefore":"...","playedMoveUci":"e2e4", ...}
{"cmd":"go","fen":"...","nodeBudget":80000,"diagnosticIsolation":"cold"}
```

The last form is the deterministic fixed-node diagnostic interface: `nodeBudget`
stops the search exactly at that node count (single-thread forced),
`diagnosticIsolation` is `cold` (fresh searcher — a prior search cannot alter the
result) or `warm` (persisted TT carries forward), and the reply carries a
`diagnostic` block with requested/consumed nodes.

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

## Documentation Map

Contracts and engineering standards:

- `docs/TEACHING_FACTS_PROTOCOL.md` — the facts contract (registry v22).
- `docs/DETECTOR_SOUNDNESS.md` — detector guard patterns, the fuzz-found
  false-positive classes, and the verification protocol.
- `docs/RESPONSIBILITIES.md` — module ownership and change checklists.
- `CLASSICAL_EVAL_EXPERIMENT.md` — the standing eval-experiment program
  (frozen N0 baseline, gates, promotion policy, tooling checklist).
- `benchmarks/N0-identity.json` — the pinned champion identity.

Promoted and experimental search/training work:

- `SEARCH_REPORT.md`
- `SEARCH_PATCHES.md`
- `RSI_LOOP_REPORT.md`
- `GEN8_TRAINING_PLAN.md`
- `benchmarks/README.md`
- `benchmarks/GENERATION_STANDARD.md`
- `benchmarks/ENGINE_STRENGTH_AUDIT.md`
- `benchmarks/BASELINE_2026-06-19.md`
- `benchmarks/CLEAN_HOLDOUT_2026-06-19.md`
- `benchmarks/CLEAN_BASELINE_2026-06-19.md`

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
