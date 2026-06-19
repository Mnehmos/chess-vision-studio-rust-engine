# Gate 3 — CP-Loss Decision-Quality Suite. The required metric: how much did
# the chosen move actually lose (SF child-eval), not just exact oracle match.
# Gen7's lesson: exact-match was flat vs gen6 while blunders collapsed 5x.
#
#   python bench_cploss.py --suite suite-fresh-100 [--exe CAND] [--net N]
#                          [--no-futility] [--depth 9 | --movetime 500]
#
# Pass: avgCP improves or neutral; p90/p95 not materially worse; bl>=200 must
# NOT increase; danger subset must not regress.
import statistics
import sys

import benchlib as B


def arg(flag, dflt=None):
    return sys.argv[sys.argv.index(flag) + 1] if flag in sys.argv else dflt


suite = B.load_suite(arg('--suite', 'suite-fresh-100'))
assert suite['oracle'], 'suite has no saved ORACLE moves'
depth = int(arg('--depth', '9'))
movetime = arg('--movetime')

configs = [B.engine_cfg('baseline', depth=depth)]
if any(fl in sys.argv for fl in ('--exe', '--net', '--no-futility', '--extra')):
    configs.append(B.engine_cfg(
        arg('--name', 'candidate'), exe=arg('--exe'), net=arg('--net'),
        futility=False if '--no-futility' in sys.argv else None,
        extra=arg('--extra', '').split(), depth=depth))

sf_depth = int(arg('--sf-depth', str(B.DEFAULT_STOCKFISH_REVIEW_DEPTH)))
sf = B.Stockfish(depth=sf_depth)
fens, orc, danger = suite['fens'], suite['oracle'], suite['danger'] or [False] * len(suite['fens'])
n = len(fens)
out = {}
print(f"# Gate 3 cp-loss — {suite['name']} (hash {suite['hash']}), "
      f"engine {'movetime ' + movetime + 'ms' if movetime else 'd' + str(depth)}, "
      f"SF-d{sf_depth} child evals")
print(f"{'config':>12s} {'match%':>7s} {'avgCP':>7s} {'medCP':>6s} {'p90':>5s} {'p95':>5s} "
      f"{'bl100':>6s} {'bl200':>6s} {'danger':>7s} {'quiet':>7s}")
for cfg in configs:
    e = B.Engine(cfg)
    moves = []
    for f in fens:
        r = e.search_time(f, int(movetime)) if movetime else e.search_depth(f)
        moves.append(r.get('uci'))
    e.close()
    losses = []
    for i in range(n):
        best = sf.child_cp(fens[i], orc[i])
        ce = sf.child_cp(fens[i], moves[i]) if moves[i] else None
        losses.append(None if best is None or ce is None else max(0, best - ce))
    xs = sorted(x for x in losses if x is not None)
    dl = [losses[i] for i in range(n) if danger[i] and losses[i] is not None]
    ql = [losses[i] for i in range(n) if not danger[i] and losses[i] is not None]
    stats = {
        'match_pct': 100 * sum(1 for i in range(n) if moves[i] == orc[i]) / n,
        'avg': statistics.mean(xs), 'median': statistics.median(xs),
        'p90': xs[int(0.90 * len(xs))], 'p95': xs[min(len(xs) - 1, int(0.95 * len(xs)))],
        'bl100_pct': 100 * sum(1 for x in xs if x >= 100) / len(xs),
        'bl200_pct': 100 * sum(1 for x in xs if x >= 200) / len(xs),
        'danger_avg': statistics.mean(dl) if dl else None,
        'quiet_avg': statistics.mean(ql) if ql else None,
    }
    out[cfg['name']] = {'stats': stats, 'moves': moves, 'losses': losses}
    fmt = lambda v: f'{v:>7.1f}' if v is not None else '      -'
    print(f"{cfg['name']:>12s} {stats['match_pct']:>6.1f}% {stats['avg']:>7.1f} "
          f"{stats['median']:>6.0f} {stats['p90']:>5.0f} {stats['p95']:>5.0f} "
          f"{stats['bl100_pct']:>5.1f}% {stats['bl200_pct']:>5.1f}% "
          f"{fmt(stats['danger_avg'])} {fmt(stats['quiet_avg'])}")
sf.close()

if len(configs) == 2:
    b, c = out['baseline']['stats'], out[configs[1]['name']]['stats']
    verdicts = [
        ('avgCP not worse', c['avg'] <= b['avg'] + 2),
        ('p90 not materially worse', c['p90'] <= b['p90'] * 1.15 + 5),
        ('bl>=200 not increased', c['bl200_pct'] <= b['bl200_pct']),
        ('danger not regressed', (c['danger_avg'] or 0) <= (b['danger_avg'] or 0) + 3),
    ]
    for name, ok in verdicts:
        print(f"  {'PASS' if ok else 'FAIL'}  {name}")
    print(f"GATE 3: {'PASS' if all(ok for _, ok in verdicts) else 'FAIL'}")

B.write_result('gate3-cploss', {
    'provenance': [B.provenance(c) for c in configs],
    'suite': {'name': suite['name'], 'hash': suite['hash']},
    'engine_budget': {'movetime_ms': movetime, 'depth': None if movetime else depth},
    'results': {k: v['stats'] for k, v in out.items()},
})
