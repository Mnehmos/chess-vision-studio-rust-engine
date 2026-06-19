# Gen 9 Training Pipeline

Gen 9 combines piece-square inputs with versioned CVS geometry features. Dataset
preparation must use an analyzer whose feature registry matches the model
metadata.

## Recursive Improvement Loop

`scripts/disagreement_selfplay.py` plays the raw and hybrid models against each
other, branches when a model violates the opponent's predicted reply, and labels
the resulting positions with Stockfish. Oracle labels default to depth 24.

Paths can be supplied through CLI options or these environment variables:

- `CVS_SF_EXE`
- `CVS_RUST_UCI`
- `CVS_OPENING_BOOK`
- `CVS_DISAGREEMENT_OUT`

`scripts/rsi_run_loop.py` deduplicates Lichess and disagreement rows against the
base corpus, normalizes cp/mate labels, prepares registry-bound shards, and
optionally trains the matrix model.

Use `--dry-run` to inspect dataset inputs without writing or launching training.
Use `--skip-training` to stop after dataset preparation.

Unverified networks require the explicit `--allow-unverified-net` development
override. Production candidates must carry the current registry hash and pass
holdout and match validation before promotion.
