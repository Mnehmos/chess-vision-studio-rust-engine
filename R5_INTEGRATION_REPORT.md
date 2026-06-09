# R5 — Rust engine integration into the app/backend path

**Status: ✅ accepted.** The Rust engine is callable from the app layer behind a
clean backend seam, the selector works, the TS legacy backend remains available
(explicitly, not hidden), integration tests pass, and the live comparison proves
identical chess at ~262× speed.

## Backend architecture

```
app / arena consumers
        │
  CvsEngineBackend  (arena/engine-backend/types.ts)
        │  id() · bestMove() · analyze() · evaluate() · dispose()
        ├── RustBackend      (arena/engine-backend/rust-backend.ts)  ← ACTIVE / default
        │     CLI subprocess bridge: cvs-bitboard-core `analyze --serve`
        │     one long-lived process per depth · FEN in → JSON out
        │     static eval via the `eval <fen>` serve command (TS-parity)
        └── TsLegacyBackend  (arena/engine-backend/ts-legacy.ts)     ← frozen reference
              wraps @cvs/engine CvsEngine (chess.js)
```

CLI subprocess is the deliberate first bridge (simplest stable). WASM or native
bindings can replace it later **without changing the seam**.

## CLI contract (`analyze` binary)

- Batch: `analyze --fens <file> --depth N [--base w.json --rung2 r.json]` →
  one JSON line per FEN.
- Serve: `analyze --serve --depth N [...]` → FEN per stdin line → JSON line:
  `{fen, uci, scoreCp, mate, pv[], depth, nodes, qNodes, qCaptures, quietExt,
  ttHits, cutoffs, timeMs}`; `eval <fen>` → `{fen, evalWhiteCp}`; `quit` exits.
- Flushed per line; errors come back as `{fen, error}` (never a crash).

## Config flag

`CVS_ENGINE_BACKEND=rust|ts` (also `legacy`→ts). **Default: `rust`**, flipped on
the strength of the GREEN R4 gate (see R4_GATE_REPORT.md: parity at d2–d4;
Rust d6 −27% avg cpLoss vs TS d4 at 32× speed; illegal=0; mate-missed=0).
Rust operating depth default: **6** (`RUST_DEFAULT_DEPTH`).

## Verification

- `arena/__tests__/engine-backend.test.ts` — **8/8 pass** (~0.7s):
  selector resolution (rust default, ts/legacy switch) · backend classes ·
  **exact eval parity through the subprocess** (rust == ts on startpos) ·
  legal best move (startpos) · forensic #549 at d4 avoids the quiet-refuted
  b3f7 · JSON protocol stable across sequential requests · no illegal move on a
  5-FEN battery · identity (engine git rev + weights id).
- `npm run engine:compare -- --fen <fen> --depth 4 --backends ts,rust` —
  live result on the forensic FEN: both pick **h7f7 @ 303cp, identical PV**;
  `sameMove: true`, `scoreDiff: 0cp`, **speedup 262×** (4458ms → 17ms).

## Commands

```
cargo build --release                      # in chess-vision-studio-rust-engine
npx vitest run arena/__tests__/engine-backend.test.ts
npm run engine:compare -- --fen "<fen>" --depth 4 --backends ts,rust
CVS_ENGINE_BACKEND=ts <anything>           # opt back into the legacy reference
```

## Known limitations / next steps

- SAN is not produced by the Rust CLI (presentation concern); callers convert
  UCI→SAN via chess.js where needed.
- One serve process per depth (cheap; a depth parameter per request would let a
  single process serve all depths — easy follow-up).
- The Lichess bot still constructs the TS `CvsEngine` directly; migrating it to
  `createEngineBackend()` is the natural next integration (gated on a gauntlet
  proof run, which the gauntlet bench now provides).
- Windows path defaults (`analyze.exe`); parameterized via `RustBackendOptions`.
