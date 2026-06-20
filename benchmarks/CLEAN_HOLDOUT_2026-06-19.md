# Clean Post-Model Holdout: 2026-06-19

## Purpose

`suite-clean-postmodel-20260619` is the first evaluation suite frozen after the
current Gen9 model artifacts. It is intended for decision-quality comparison,
not training.

## Source Isolation

- Source: 40 ChessVisionStudioEng Lichess games.
- Game window: after `2026-06-18T19:43:00.475Z`, the newest current-model
  artifact timestamp.
- Source PGN hash: `81ccf8c42ed089db07e777d7d72676e5d64522d9a6b2d79ce87b30aceb5fabc6`.
- Whole-game reservation: 3,031 unique legal positions from all 40 games.
- Reservation enforcement: the Gen9 RSI importer refuses to train if a reserved
  position already exists in its base corpus and skips reserved incoming rows.

## Contamination Audit

The builder compared 2,720 post-ply-12 candidates using the first four FEN
fields against:

- 7,483,553 Gen7 labeled rows.
- 6,159,020 Gen8 broad-corpus rows.
- 5,041,938 Gen8-v2 final training rows.
- 7,861,317 Gen9 training rows.
- 9,999 Gen9 ranker/residual delta rows.
- 300 positions from the three historical benchmark suites.

Forty-four common positions appeared in the broad Gen8 corpus and were removed.
No candidate overlap was found in the Gen8-v2 final corpus, Gen9 corpus, Gen7
labels, Gen9 deltas, or prior benchmark suites.

## Selection And Labels

- Deterministic salt: `cvs-clean-holdout-20260619-v1`.
- Initial sample: 120 positions, at most four per game and ten plies apart.
- Stockfish: native AVX2 binary, depth 24, one thread, 256 MB hash, MultiPV 2.
- Final competitive sample: 92 positions.
- Removed: 28 positions with `abs(score) > 3000` because forced mates and
  already-decided positions provide weak move-selection discrimination.
- Phase mix: 30 opening, 57 middlegame, 5 endgame.
- Danger positions: 35.
- Median absolute evaluation: 327 cp; p90: 724 cp.

The endgame slice is intentionally reported as insufficient. It must be
supplemented by a separate source-isolated endgame suite rather than by
loosening this suite's provenance rules.

## Frozen Artifacts

- `suite-clean-postmodel-20260619.txt`
- `suite-clean-postmodel-20260619.source.json`
- `suite-clean-postmodel-20260619.moves.json`
- `suite-clean-postmodel-20260619.danger.json`
- `suite-clean-postmodel-20260619.labels.json`
- `suite-clean-postmodel-20260619.rejected.json`
- `holdout-reserved-20260619.txt`

Decision: ACCEPT AS CLEAN DECISION HOLDOUT
