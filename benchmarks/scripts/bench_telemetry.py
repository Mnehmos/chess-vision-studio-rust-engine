# Search telemetry aggregate. Use before stacking pruning patches.
#
#   python bench_telemetry.py --suite canonical --base-exe target/release/analyze.exe --exe target/release/analyze.exe --extra "--rfp"
#   python bench_telemetry.py --suite suite-hard-100 --base-exe target/release/analyze.exe --exe target/release/analyze.exe --extra "--rfp --lmp"
import json
import sys

import benchlib as B


def arg(flag, dflt=None):
    return sys.argv[sys.argv.index(flag) + 1] if flag in sys.argv else dflt


def pct(num, den):
    return 0.0 if not den else 100.0 * num / den


def avg(num, den):
    return 0.0 if not den else num / den


def warn(msg):
    print(f"[warn] {msg}", file=sys.stderr)


def positions_for(name):
    if name == 'canonical':
        canon = json.load(open(f'{B.SUITES}/canonical.json'))['positions']
        return [(p['name'], p['fen']) for p in canon]
    suite = B.load_suite(name)
    return [(f'{name}-{i:03d}', fen) for i, fen in enumerate(suite['fens'])]


def add(a, t):
    for k, v in t.items():
        # Counters sum across positions; precomputed ratios (xxxPct, avgXxx,
        # effectiveBranching) do NOT — summing rates produced >100% artifacts
        # in the persisted rows. Ratios are recomputed from counters at print.
        if isinstance(v, (int, float)) and 'Pct' not in k and not k.startswith('avg') \
                and 'Branching' not in k:
            a[k] = a.get(k, 0) + v


def telemetry_from_result(r):
    t = dict(r.get('telemetry') or {})
    legacy = not bool(t)
    fallback = {
        'nodes': 'nodes',
        'qNodes': 'qNodes',
        'qCaptures': 'qCaptures',
        'quietExt': 'quietExt',
        'ttHits': 'ttHits',
        'cutoffs': 'cutoffs',
        'nullCutoffs': 'nullCutoffs',
        'timeMs': 'timeMs',
    }
    for src, dst in fallback.items():
        if dst not in t and src in r:
            t[dst] = r[src]
    return t, legacy


suite_name = arg('--suite', 'canonical')
depth = int(arg('--depth', '8'))
movetime = arg('--movetime')
positions = positions_for(suite_name)

base_cfg = B.engine_cfg(
    arg('--base-name', 'baseline'),
    exe=arg('--base-exe'),
    net=arg('--base-net'),
    futility=False if '--base-no-futility' in sys.argv else None,
    extra=arg('--base-extra', '').split(),
    depth=depth,
)
configs = [base_cfg]
if any(fl in sys.argv for fl in ('--exe', '--net', '--no-futility', '--extra')):
    configs.append(B.engine_cfg(
        arg('--name', 'candidate'), exe=arg('--exe'), net=arg('--net'),
        futility=False if '--no-futility' in sys.argv else None,
        extra=arg('--extra', '').split(), depth=depth))

rows = {}
for cfg in configs:
    e = B.Engine(cfg)
    agg = {}
    legacy_rows = 0
    for _, fen in positions:
        r = e.search_time(fen, int(movetime)) if movetime else e.search_depth(fen)
        t, legacy = telemetry_from_result(r)
        legacy_rows += int(legacy)
        add(agg, t)
    e.close()
    if legacy_rows:
        warn(
            f"{cfg['name']} emitted legacy top-level telemetry for "
            f"{legacy_rows}/{len(positions)} positions; new cut/attempt fields "
            "will be missing. Pass --base-exe target/release/analyze.exe for "
            "current-build baselines."
        )
    rows[cfg['name']] = agg

print(f"# telemetry - {suite_name}, {'movetime ' + movetime + 'ms' if movetime else 'd' + str(depth)}")
print(
    f"{'config':>12s} {'nodes':>11s} {'q%':>6s} {'ebf':>6s} "
    f"{'ttHit%':>7s} {'ttCut%':>7s} {'hashCut%':>8s} {'1stCut%':>8s} "
    f"{'cutIdx':>7s} {'rfp c/a':>13s} {'null c/a':>13s} {'fut c/a':>13s} "
    f"{'lmp c/a':>13s} {'see c/a':>13s} {'delta c/a':>13s}"
)
for cfg in configs:
    a = rows[cfg['name']]
    line = (
        f"{cfg['name']:>12s} "
        f"{int(a.get('nodes', 0)):>11d} "
        f"{pct(a.get('qNodes', 0), a.get('nodes', 0)):>5.1f}% "
        f"{avg(a.get('searchedMoves', 0), a.get('legalMoveNodes', 0)):>6.2f} "
        f"{pct(a.get('ttHits', 0), a.get('ttProbes', 0)):>6.1f}% "
        f"{pct(a.get('ttCutoffs', 0), a.get('ttProbes', 0)):>6.1f}% "
        f"{pct(a.get('hashMoveCutoffs', 0), a.get('cutoffs', 0)):>7.1f}% "
        f"{pct(a.get('firstMoveCutoffs', 0), a.get('cutoffs', 0)):>7.1f}% "
        f"{avg(a.get('cutoffMoveIndexSum', 0), a.get('cutoffMoveIndexCount', 0)):>7.2f} "
        f"{int(a.get('rfpCutoffs', 0)):>5d}/{int(a.get('rfpAttempts', 0)):<7d} "
        f"{int(a.get('nullCutoffs', 0)):>5d}/{int(a.get('nullAttempts', 0)):<7d} "
        f"{int(a.get('futilitySkips', 0)):>5d}/{int(a.get('futilityAttempts', 0)):<7d} "
        f"{int(a.get('lmpSkips', 0)):>5d}/{int(a.get('lmpAttempts', 0)):<7d} "
        f"{int(a.get('seePruneSkips', 0)):>5d}/{int(a.get('seePruneAttempts', 0)):<7d} "
        f"{int(a.get('deltaSkips', 0)):>5d}/{int(a.get('deltaAttempts', 0)):<7d}"
    )
    print(line)

B.write_result('telemetry', {
    'provenance': [B.provenance(c) for c in configs],
    'suite': suite_name,
    'positions': [name for name, _ in positions],
    'engine_budget': {'movetime_ms': movetime, 'depth': None if movetime else depth},
    'rows': rows,
})
