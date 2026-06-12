#!/usr/bin/env python3
"""Assemble the gen8 training corpus from three tiers into one shuffled file
of {fen, cp, res} rows with WHITE-POV cp (what train-nnue.py's encode expects).

  Tier A broad:    public Lichess eval d20            (already white-POV cp)
  Tier B endgame:  public Lichess eval d20, <=12 pcs  (already white-POV cp)
  Tier C booster:  our weak-point d20 relabel         (sfCp is STM-POV -> flip)

Booster is oversampled (--boost-mult) so the ~8k failure positions actually
move the gradient against 6M broad rows. Train with --lambda 1.0 (these rows
carry no game result; res is a sentinel and ignored at lambda 1).

  python assemble_gen8.py out.jsonl \
     --broad f:/tools/gen8-public-lichess-d20.jsonl \
     --endgame f:/tools/gen8-public-endgame-d20.jsonl \
     --booster-glob "f:/tools/gen8-wp-labeled*.jsonl" [--boost-mult 20]
"""
import glob
import json
import random
import sys

random.seed(8)


def arg(flag, dflt=None):
    return sys.argv[sys.argv.index(flag) + 1] if flag in sys.argv else dflt


out_path = sys.argv[1]
broad = arg('--broad')
endgame = arg('--endgame')
booster_glob = arg('--booster-glob')
boost_mult = int(arg('--boost-mult', '20'))

rows = []


def take_public(path, tag):
    n = 0
    with open(path, encoding='utf8') as f:
        for line in f:
            try:
                r = json.loads(line)
                rows.append((r['fen'], int(r['cp'])))  # already white-POV
                n += 1
            except Exception:
                pass
    print(f'  {tag}: {n} rows')


def take_booster(pattern, mult):
    n = 0
    for fp in glob.glob(pattern):
        with open(fp, encoding='utf8') as f:
            for line in f:
                try:
                    r = json.loads(line)
                    cp = r.get('sfCp')
                    if cp is None:
                        # mate label -> mate-scale cp
                        m = r.get('sfMate')
                        if m is None:
                            continue
                        cp = 10000 if m > 0 else -10000
                    white = cp if r['fen'].split()[1] == 'w' else -cp
                    for _ in range(mult):
                        rows.append((r['fen'], int(white)))
                    n += 1
                except Exception:
                    pass
    print(f'  booster: {n} unique x{mult}')


if broad:
    take_public(broad, 'broad')
if endgame:
    take_public(endgame, 'endgame')
if booster_glob:
    take_booster(booster_glob, boost_mult)

random.shuffle(rows)
with open(out_path, 'w', encoding='utf8') as w:
    for fen, cp in rows:
        w.write(json.dumps({'fen': fen, 'cp': cp, 'res': 0.5}) + '\n')
print(f'wrote {len(rows)} rows -> {out_path}')
