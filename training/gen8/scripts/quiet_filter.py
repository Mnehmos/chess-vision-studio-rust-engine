#!/usr/bin/env python3
"""Quiet-filter a training corpus — the step gen8-v1 skipped (and regressed).

A position is QUIET if the side to move is not in check and, in --strict mode,
no capture wins material by SEE. NNUE evals are only stable on quiet positions;
training on mid-tactic / in-check nodes teaches the net noise.

  python quiet_filter.py in.jsonl out.jsonl [--strict] [--cap N]

Rows pass through unchanged (any schema with a 'fen'); only non-quiet rows are
dropped. Fast mode (default) = in-check filter only (~cheap). Strict adds a
SEE-positive-capture test (slower, python-chess).
"""
import json
import sys

import chess


def arg(flag, dflt=None):
    return sys.argv[sys.argv.index(flag) + 1] if flag in sys.argv else dflt


inp, out = sys.argv[1], sys.argv[2]
strict = '--strict' in sys.argv
cap = int(arg('--cap', '0'))

# Minimal SEE: is there a capture whose immediate material gain (victim minus
# attacker, refined by the recapture) is positive? Conservative — only filters
# obviously-tactical nodes, which is what we want.
VAL = {chess.PAWN: 100, chess.KNIGHT: 320, chess.BISHOP: 330,
       chess.ROOK: 500, chess.QUEEN: 900, chess.KING: 20000}


def has_winning_capture(board):
    for mv in board.legal_moves:
        if not board.is_capture(mv):
            continue
        victim = board.piece_type_at(mv.to_square)
        if victim is None:  # en passant
            victim = chess.PAWN
        attacker = board.piece_type_at(mv.from_square)
        # Cheap SEE proxy: winning if victim >= attacker, or target undefended.
        if VAL[victim] >= VAL[attacker]:
            return True
        if not board.is_attacked_by(not board.turn, mv.to_square):
            return True
    return False


kept = dropped = 0
with open(inp, encoding='utf8') as f, open(out, 'w', encoding='utf8') as w:
    for line in f:
        try:
            row = json.loads(line)
            board = chess.Board(row['fen'])
        except Exception:
            dropped += 1
            continue
        if board.is_check() or (strict and has_winning_capture(board)):
            dropped += 1
            continue
        w.write(line if line.endswith('\n') else line + '\n')
        kept += 1
        if cap and kept >= cap:
            break
        if (kept + dropped) % 500000 == 0:
            print(f'  {kept} kept / {dropped} dropped', flush=True)
print(f'quiet-filter: kept {kept}, dropped {dropped} '
      f'({dropped * 100 // max(1, kept + dropped)}% non-quiet){" [strict]" if strict else ""}')
