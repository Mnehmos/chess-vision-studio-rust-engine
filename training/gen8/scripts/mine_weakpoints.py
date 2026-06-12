#!/usr/bin/env python3
"""Mine gen8 Tier-B hard-booster seeds from our own annotated game PGNs.

Our SPRT/screen PGNs carry per-move evals ({+0.61/7 0.21s}) for the side to
move. We replay each game and snapshot FENs at two failure modes:

  collapse   — our eval fell >= DROP cp in a single one of OUR moves
               (tactical miss: the position our eval over-rated, snapshot
               taken BEFORE the move so the label targets the truth there)
  noconvert  — our eval peaked >= WIN cp but the game ended draw/loss for us
               (conversion failure: snapshot at the peak)

Output: {fen, reason, ourCp} JSONL, deduped by FEN. These are exactly the
positions public eval data won't contain — they are how OUR engine fails.
Run downstream: sf-relabel-worker.py at d20 to get the booster labels.

  python mine_weakpoints.py "f:/tools/*.pgn" out.jsonl [--drop 250] [--win 200]
"""
import glob
import json
import re
import sys

import chess
import chess.pgn

DROP = int(sys.argv[sys.argv.index('--drop') + 1]) if '--drop' in sys.argv else 250
WIN = int(sys.argv[sys.argv.index('--win') + 1]) if '--win' in sys.argv else 200
pattern, out_path = sys.argv[1], sys.argv[2]

EVAL_RE = re.compile(r'([+-]?\d+\.\d+|[+-]?M\d+)')


def cp_of(comment):
    m = EVAL_RE.search(comment or '')
    if not m:
        return None
    tok = m.group(1)
    if 'M' in tok:
        return 10000 if tok[0] != '-' else -10000
    return int(round(float(tok) * 100))


seen = set()
kept = {'collapse': 0, 'noconvert': 0}
files = glob.glob(pattern)
out = open(out_path, 'w', encoding='utf8')
for fp in files:
    with open(fp, encoding='utf8', errors='ignore') as fh:
        while True:
            game = chess.pgn.read_game(fh)
            if game is None:
                break
            white = game.headers.get('White', '')
            result = game.headers.get('Result', '*')
            # Which color are WE? cand/ttps/histlmr/etc. vs base — treat the
            # non-"base" side as ours; if both unknown, skip color logic.
            we_white = white != 'base'
            board = game.board()
            our_cps = []  # (ply_node_board_fen_before, cp_after_our_move)
            prev_our_cp = None
            peak = (-99999, None)
            node = game
            collapse_fens = []
            while node.variations:
                nxt = node.variation(0)
                mover_is_white = board.turn == chess.WHITE
                fen_before = board.fen()
                board.push(nxt.move)
                cp = cp_of(nxt.comment)
                ours = (mover_is_white == we_white)
                if ours and cp is not None:
                    # eval is from the mover's POV after the move
                    if prev_our_cp is not None and prev_our_cp - cp >= DROP:
                        collapse_fens.append((fen_before, prev_our_cp))
                    prev_our_cp = cp
                    if cp > peak[0]:
                        peak = (cp, fen_before)
                node = nxt
            # collapse seeds
            for fen, c in collapse_fens:
                if fen not in seen:
                    seen.add(fen)
                    out.write(json.dumps({'fen': fen, 'reason': 'collapse', 'ourCp': c}) + '\n')
                    kept['collapse'] += 1
            # no-convert seed: peaked winning, but we didn't win
            we_won = (result == '1-0' and we_white) or (result == '0-1' and not we_white)
            if peak[0] >= WIN and not we_won and peak[1] and peak[1] not in seen:
                seen.add(peak[1])
                out.write(json.dumps({'fen': peak[1], 'reason': 'noconvert', 'ourCp': peak[0]}) + '\n')
                kept['noconvert'] += 1
out.close()
print(f'files {len(files)}, unique seeds {len(seen)}: {kept}')
