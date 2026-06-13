#!/usr/bin/env python3
"""Self-play corpus generator — grow the RIGHT-distribution training set.

Plays the champion against itself over the opening book (diversity from varied
openings + a little eval noise from the clock), writes PGN, then extracts quiet
midgame/endgame positions our engine actually reaches. Those positions are the
correct broad-corpus distribution (the v1 lesson) and get depth-20 relabeled
downstream — no labels are produced here, only positions.

  python selfplay_gen.py --games 400 [--tc 5+0.05] [--out selfplay.pgn]
  # then: extract_pgn_positions.py selfplay.pgn -> fens, then sf-relabel d20

Defaults target the deployed champion (gen8-v2 + full search stack). Keep
--concurrency modest while the d20 relabel holds the other cores.
"""
import subprocess
import sys

CC = 'F:/tools/cutechess/cutechess-1.3.1-win64/cutechess-cli.exe'
BOOK = 'F:/tools/openings.epd'
UCI = 'f:/tools/cvs-baselines/uci-gen8v2-champion.exe'
NET = 'f:/Github/chess-vision-studio/arena/out/gen8-raw-h256-v2.json'
BASE = 'f:/Github/chess-vision-studio/arena/out/value-weights-mixed.json'
RUNG2 = 'f:/Github/chess-vision-studio/arena/out/rung2-weights-mixed.json'
FLAGS = ['--futility', '--rfp', '--tt-prune-store', '--qtt', '--histmalus', '--histlmr']


def arg(flag, dflt=None):
    return sys.argv[sys.argv.index(flag) + 1] if flag in sys.argv else dflt


def main():
    games = arg('--games', '400')
    tc = arg('--tc', '5+0.05')
    conc = arg('--concurrency', '2')
    out = arg('--out', 'f:/tools/selfplay-gen8v2.pgn')
    eng = ['cmd=' + UCI, 'arg=--base', f'arg={BASE}', 'arg=--rung2', f'arg={RUNG2}',
           'arg=--nnue', f'arg={NET}'] + [f'arg={f}' for f in FLAGS]
    cmd = [CC,
           '-engine', 'name=cvs-a', *eng,
           '-engine', 'name=cvs-b', *eng,
           '-each', 'proto=uci', f'tc={tc}',
           '-games', games, '-repeat', '-concurrency', conc,
           '-openings', f'file={BOOK}', 'format=epd', 'order=random',
           '-pgnout', out]
    print('self-play:', ' '.join(cmd[:6]), '...', f'-> {out}', flush=True)
    subprocess.run(cmd)
    print(f'self-play done -> {out}; next: extract_pgn_positions.py {out}')


if __name__ == '__main__':
    main()
