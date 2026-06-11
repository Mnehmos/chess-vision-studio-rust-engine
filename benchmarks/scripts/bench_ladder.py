# Gate 2 — Movetime Depth/NPS Ladder. Practical depth + throughput vs snapshot.
#
#   python bench_ladder.py [--exe CAND] [--net N] [--no-futility]
#                          [--threads 1,4,8] [--times 5,50,500,5000]
#
# Pass: speed patches improve NPS or time-to-depth; pruning patches improve
# reached depth at equal movetime. If NPS is similar but depth lags, inspect
# selectivity / ordering / TT / qsearch — raw NPS is no longer the bottleneck.
import json
import sys

import benchlib as B


def arg(flag, dflt=None):
    return sys.argv[sys.argv.index(flag) + 1] if flag in sys.argv else dflt


times = [int(t) for t in arg('--times', '5,10,25,50,100,250,500,1000,2500,5000').split(',')]
threads = [int(t) for t in arg('--threads', '1,4,8').split(',')]
canon = json.load(open(f'{B.SUITES}/canonical.json'))
positions = [p for p in canon['positions'] if p['name'] in canon['ladder_subset']]

configs = [B.engine_cfg('baseline')]
if '--exe' in sys.argv or '--net' in sys.argv or '--no-futility' in sys.argv:
    configs.append(B.engine_cfg(
        arg('--name', 'candidate'), exe=arg('--exe'), net=arg('--net'),
        futility=False if '--no-futility' in sys.argv else None))

table = {}
for cfg in configs:
    for t in threads:
        e = B.Engine({**cfg, 'threads': t, 'depth': 30})
        for ms in times:
            ds, ns, nodes = [], [], 0
            for p in positions:
                r = e.search_time(p['fen'], ms)
                ds.append(r.get('depth', 0))
                el = max(1, r.get('timeMs', ms))
                ns.append(r.get('nodes', 0) / el / 1000)
                nodes += r.get('nodes', 0)
            key = f"{cfg['name']}|{t}T|{ms}ms"
            table[key] = {'avg_depth': sum(ds) / len(ds), 'avg_mnps': sum(ns) / len(ns),
                          'nodes': nodes}
        e.close()

print(f"{'config':>12s} {'thr':>4s} " + ' '.join(f'{ms:>11d}ms' for ms in times))
for cfg in configs:
    for t in threads:
        cells = []
        for ms in times:
            v = table[f"{cfg['name']}|{t}T|{ms}ms"]
            cells.append(f"d{v['avg_depth']:>4.1f}/{v['avg_mnps']:.2f}M")
        print(f"{cfg['name']:>12s} {t:>3d}T " + ' '.join(f'{c:>13s}' for c in cells))

B.write_result('gate2-ladder', {
    'provenance': [B.provenance(c) for c in configs],
    'times_ms': times, 'threads': threads,
    'positions': [p['name'] for p in positions], 'table': table,
})
