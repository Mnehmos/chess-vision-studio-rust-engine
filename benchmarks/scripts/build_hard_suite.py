# Build suite-hard-100: positions where the SNAPSHOT engine itself loses
# >=50cp vs SF-d24 at d7 — mined from the self-play corpus. "Hard" is defined
# relative to the frozen baseline, so it concentrates exactly the failures
# future candidates should fix. Composition skews danger/tactical naturally.
#
#   python build_hard_suite.py [--target 100] [--source f:/tmp/nnue-all.jsonl]
import json
import os
import sys

import benchlib as B


def arg(flag, dflt=None):
    return sys.argv[sys.argv.index(flag) + 1] if flag in sys.argv else dflt


target = int(arg('--target', '100'))
source = arg('--source', 'f:/tmp/nnue-all.jsonl')
sf_depth = int(arg('--sf-depth', str(B.DEFAULT_STOCKFISH_REVIEW_DEPTH)))
THRESH = 50  # cp loss for "hard"
existing = set()
for s in ('suite-dev-100', 'suite-fresh-100'):
    p = os.path.join(B.SUITES, s + '.txt')
    if os.path.exists(p):
        existing |= {l.strip() for l in open(p) if l.strip()}

eng = B.Engine(B.engine_cfg('baseline', depth=7))
sf = B.Stockfish(depth=sf_depth)
hard, oracle, scanned = [], [], 0
with open(source, encoding='utf8') as f:
    for i, line in enumerate(f):
        if len(hard) >= target:
            break
        if i % 4391 != 77:  # spaced sample, disjoint stride from dev/fresh builders
            continue
        try:
            fen = json.loads(line)['fen']
        except Exception:
            continue
        if fen in existing or fen.split()[0].count('K') != 1 or fen.split()[0].count('k') != 1:
            continue
        scanned += 1
        r = eng.search_depth(fen)
        mv = r.get('uci')
        if not mv:
            continue
        _, sf_best = sf.go(fen, depth=sf_depth)
        best = sf.child_cp(fen, sf_best)
        mine = sf.child_cp(fen, mv)
        if best is None or mine is None:
            continue
        if max(0, best - mine) >= THRESH:
            hard.append(fen)
            oracle.append(sf_best)
            if len(hard) % 10 == 0:
                print(f'{len(hard)}/{target} hard ({scanned} scanned)', flush=True)
eng.close()
sf.close()

open(os.path.join(B.SUITES, 'suite-hard-100.txt'), 'w').write('\n'.join(hard) + '\n')
json.dump({'ORACLE': oracle,
           'note': f'mined {len(hard)} positions where snapshot d7 loses >={THRESH}cp vs SF-d{sf_depth} oracle/scoring'},
          open(os.path.join(B.SUITES, 'suite-hard-100.moves.json'), 'w'))
print(f'wrote suite-hard-100: {len(hard)} positions from {scanned} scanned '
      f'(hard rate {len(hard)/max(1,scanned)*100:.1f}%)')
print('hash:', B.sha256(os.path.join(B.SUITES, 'suite-hard-100.txt')))
