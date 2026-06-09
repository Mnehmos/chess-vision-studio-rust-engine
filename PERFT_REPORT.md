# CVS Bitboard Core v0 — perft correctness + speed report

**Goal of v0:** prove a *correct* and *fast* legal move generator + perft as the Rust
engine-core seed for Chess Vision Studio. It does **not** replace the current
chess.js-based TS engine — it's the foundation for a future Rust search/SEE/eval.

## Status: ✅ correct, ~1000× faster movegen than chess.js

- **Correctness:** all 6 canonical perft positions pass at every tested depth,
  including the deep checks (startpos d5 = 4,865,609; Kiwipete d4 = 4,085,603).
  This exercises normal moves, captures, castling, en passant, promotions, and
  check/evasion.
- **Speed:** **~30–36 million nodes/sec** (release), vs chess.js **~16–30 thousand
  nodes/sec** — roughly **1,000×** faster move generation.

## Perft correctness (Rust bitboard core, release)

| Position | Depth | Nodes | Expected | OK | Nodes/s |
|---|---:|---:|---:|:--:|---:|
| startpos | 4 | 197,281 | 197,281 | ✓ | ~31.3M |
| startpos | 5 | 4,865,609 | 4,865,609 | ✓ | ~31.6M |
| kiwipete | 3 | 97,862 | 97,862 | ✓ | ~32.9M |
| kiwipete | 4 | 4,085,603 | 4,085,603 | ✓ | ~31.5M |
| position3 (ep/checks) | 4 | 43,238 | 43,238 | ✓ | ~25.1M |
| position3 (ep/checks) | 5 | 674,624 | 674,624 | ✓ | ~28.7M |
| position4 (castle/promo) | 4 | 422,333 | 422,333 | ✓ | ~22.6M |
| position5 (promo) | 4 | 2,103,487 | 2,103,487 | ✓ | ~31.4M |
| position6 | 4 | 3,894,594 | 3,894,594 | ✓ | ~34.2M |

Full table (all depths 1–5) is printed by `cargo run --release --bin perft`.

## nodes/sec benchmark (startpos)

| Depth | Nodes | Nodes/s |
|---:|---:|---:|
| 4 | 197,281 | ~31.3M |
| 5 | 4,865,609 | ~31.6M |

## chess.js baseline (TS engine's current movegen)

Measured via `npm run perft:chessjs` (chess.js `moves({verbose})` + `move`/`undo`):

| Position | Depth | Nodes | Nodes/s |
|---|---:|---:|---:|
| startpos | 4 | 197,281 | ~30,046 |
| kiwipete | 3 | 97,862 | ~16,503 |

(Measured while the SF-scored gate was running, so this is a conservative lower
bound — uncontended chess.js would be somewhat faster, but not close to bitboard.)

## Speedup

| Position | Bitboard nps | chess.js nps | Speedup |
|---|---:|---:|---:|
| startpos | ~31.6M | ~30k | **~1,050×** |
| kiwipete | ~31.5M | ~16.5k | **~1,900×** |

## Why this matters

The TS-side perf pass found the d4 search bottleneck is **chess.js move generation**
(verbose-SAN move gen = make/unmake + check-test per move, at ~2k nps inside search),
which a semantics-preserving tweak can only shave ~5%. A bitboard core does movegen
~1,000× faster — that is the headroom for *materially* deeper/faster search later,
without sacrificing capability.

## What v0 implements

- Bitboard board representation (LERF), incremental occupancy.
- FEN → bitboards.
- **Legal** move generation: normal, captures, castling (with path-safety), en
  passant, promotions, check evasions (pseudo-legal + make/king-attack/unmake filter).
- Exact make / unmake (undo stack: captured piece, castling, ep, halfmove).
- Perft + perft-divide.
- Perft test suite (6 canonical positions) + nodes/sec benchmark binary.

## Not in v0 (deliberately)

- Search, SEE, eval (next Rust rungs).
- Magic bitboards (classical ray attacks for now — already ~30M nps; magics would
  push higher when search needs it).
- Pin-aware legal generation (make/unmake legality is correct and fast enough here).
- **No change to the current CVS (chess.js) engine or its search path.**

## Try it

```
cargo test                         # perft correctness suite (6/6)
cargo run --release --bin perft    # full perft + nodes/sec report
cargo run --release --bin perft -- "<fen>" 5   # perft divide for one position
```
