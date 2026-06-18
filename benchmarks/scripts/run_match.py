import subprocess
import sys
import os
import benchlib as B

CC = 'F:/tools/cutechess/cutechess-1.3.1-win64/cutechess-cli.exe'
BOOK = 'F:/tools/openings.epd'

def arg(flag, dflt=None):
    if flag in sys.argv:
        idx = sys.argv.index(flag)
        if idx + 1 < len(sys.argv):
            return sys.argv[idx + 1]
    return dflt

def resolve_path(p):
    if p and os.path.exists(p):
        return os.path.abspath(p)
    return p

def resolve_args(args_str):
    return ' '.join(resolve_path(a) for a in args_str.split() if a)

name1 = arg('--name1', 'Engine1')
exe1 = resolve_path(arg('--exe1', 'target/release/uci.exe'))
args1 = resolve_args(arg('--args1', ''))

name2 = arg('--name2', 'Engine2')
exe2 = resolve_path(arg('--exe2', 'target/release/uci.exe'))
args2 = resolve_args(arg('--args2', ''))

games = int(arg('--games', '100'))
tc = arg('--tc', '5+0.05')
conc = arg('--conc', '4')
pgn = arg('--pgn', 'f:/tools/match-latest.pgn')

cmd = [
    CC,
    '-engine', f'name={name1}', f'cmd={exe1}', *[f'arg={a}' for a in args1.split() if a],
    '-engine', f'name={name2}', f'cmd={exe2}', *[f'arg={a}' for a in args2.split() if a],
    '-each', 'proto=uci', f'tc={tc}',
    '-games', str(games), '-repeat', '-concurrency', str(conc),
    '-openings', f'file={BOOK}', 'format=epd', 'order=random',
    '-pgnout', pgn
]

print(f"Running Matchup: {name1} vs {name2}")
print(f"Command: {' '.join(cmd)}")
print("-" * 60)

p = subprocess.Popen(cmd, stdout=subprocess.PIPE, stderr=subprocess.STDOUT, text=True, bufsize=1)
out_lines = []
for line in p.stdout:
    print(line, end='', flush=True)
    out_lines.append(line)
p.wait()
out = ''.join(out_lines)

# Parse WLD results
W = L = D = 0
if os.path.exists(pgn):
    try:
        g = open(pgn, encoding='utf-8', errors='ignore').read().split('[Event ')
        for x in g[1:]:
            cw = f'[White "{name1}"]' in x
            if '[Result "1-0"]' in x:
                W, L = (W + 1, L) if cw else (W, L + 1)
            elif '[Result "0-1"]' in x:
                W, L = (W, L + 1) if cw else (W + 1, L)
            elif '[Result "1/2-1/2"]' in x:
                D += 1
    except Exception as e:
        print(f"Error parsing PGN: {e}")

n = W + L + D
if n > 0:
    score = (W + D / 2) / n * 100
    print(f"\nFinal Result: {name1} vs {name2}  +{W} -{L} ={D} ({n} games) {score:.1f}%")
else:
    print(f"\nNo games completed or PGN parser failed.")
