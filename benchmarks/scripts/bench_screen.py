# Gate 5 — Cutechess Live-Dev Screen. 20 games, repeated openings. This is a
# SCREEN, not proof: never quote precise Elo from 20 games.
#
#   python bench_screen.py --cand-args "--futility --rfp" [--base-args "--futility"]
#                          [--games 20] [--tc 5+0.05] [--conc 1]
#
# Labels: >=60% green | 50-60% neutral | 40-50% suspicious (read PGNs) |
# <40% red. Pruning patches: minimum 10/20, preferred 11.5/20, red flag 8/20.
import subprocess
import sys

import benchlib as B

CC = 'F:/tools/cutechess/cutechess-1.3.1-win64/cutechess-cli.exe'
BOOK = 'F:/tools/openings.epd'


def arg(flag, dflt=None):
    return sys.argv[sys.argv.index(flag) + 1] if flag in sys.argv else dflt


games = int(arg('--games', '20'))
tc = arg('--tc', '5+0.05')
conc = arg('--conc', '1')
cand_args = arg('--cand-args', '').split()
base_args = arg('--base-args', '--futility' if B.BASELINE['futility'] else '').split()
exe = arg('--exe', B.BASELINE['uci_exe'])
pgn = arg('--pgn', 'f:/tools/screen-latest.pgn')

w = ['arg=--base', f"arg={B.BASELINE['base_weights']}",
     'arg=--rung2', f"arg={B.BASELINE['rung2_weights']}",
     'arg=--nnue', f"arg={B.BASELINE['net']}"]
cmd = [CC,
       '-engine', 'name=cand', f'cmd={exe}', *w, *[f'arg={a}' for a in cand_args],
       '-engine', 'name=base', f'cmd={exe}', *w, *[f'arg={a}' for a in base_args],
       '-each', 'proto=uci', f'tc={tc}',
       '-games', str(games), '-repeat', '-concurrency', conc,
       '-openings', f'file={BOOK}', 'format=epd', 'order=random',
       '-pgnout', pgn]
print('running:', ' '.join(cmd), file=sys.stderr)
p = subprocess.Popen(cmd, stdout=subprocess.PIPE, stderr=subprocess.STDOUT, text=True, bufsize=1)
out_lines = []
for line in p.stdout:
    print(line, end='', flush=True)
    out_lines.append(line)
p.wait()
out = ''.join(out_lines)

wl = [l for l in out.splitlines() if 'Score of' in l or 'Elo difference' in l]
print('\n'.join(wl[-4:]))
g = open(pgn, encoding='utf8', errors='ignore').read().split('[Event ')
W = L = D = 0
for x in g[1:]:
    cw = '[White "cand"]' in x
    if '[Result "1-0"]' in x:
        W, L = (W + 1, L) if cw else (W, L + 1)
    elif '[Result "0-1"]' in x:
        W, L = (W, L + 1) if cw else (W + 1, L)
    elif '[Result "1/2-1/2"]' in x:
        D += 1
n = W + L + D
score = (W + D / 2) / n * 100 if n else 0
label = ('GREEN — continue testing / live-dev trial allowed' if score >= 60 else
         'NEUTRAL — no obvious regression' if score >= 50 else
         'SUSPICIOUS — inspect PGNs before continuing' if score >= 40 else
         'RED — reject or debug before more testing')
print(f'\ncand vs base: +{W} -{L} ={D} ({n} games) {score:.1f}%  ->  {label}')
print('Passed/failed the 20-game live-dev screen. This is NOT an Elo proof; '
      'it only shows whether there is an obvious strength regression.')

B.write_result('gate5-screen', {
    'provenance': B.provenance(B.engine_cfg('screen', exe=exe)),
    'cand_args': cand_args, 'base_args': base_args, 'tc': tc, 'conc': conc,
    'wld': [W, L, D], 'score_pct': score, 'label': label.split(' — ')[0],
    'pgn': pgn,
})
