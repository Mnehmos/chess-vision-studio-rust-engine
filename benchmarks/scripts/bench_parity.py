# Gate 1 — Fixed-Depth Sanity / Parity. Candidate vs snapshot on the canonical
# positions at fixed depths; catches bugs and accidental search-shape changes.
#
#   python bench_parity.py --exe CAND.exe [--net N] [--no-futility] [--depths 6,7,8]
#   python bench_parity.py                              # snapshot self-record
#
# Speed/refactor pass: same move, same score, same nodes, same PV; time/NPS not
# materially worse. Pruning/search pass: differences allowed, follow with Gate 3.
import json
import sys

import benchlib as B


def arg(flag, dflt=None):
    return sys.argv[sys.argv.index(flag) + 1] if flag in sys.argv else dflt


depths = [int(d) for d in arg('--depths', '6,7,8').split(',')]
canon = json.load(open(f'{B.SUITES}/canonical.json'))['positions']

configs = [B.engine_cfg('baseline')]
if '--exe' in sys.argv or '--net' in sys.argv or '--no-futility' in sys.argv:
    configs.append(B.engine_cfg(
        arg('--name', 'candidate'), exe=arg('--exe'), net=arg('--net'),
        futility=False if '--no-futility' in sys.argv else None))

rows = {c['name']: {} for c in configs}
for cfg in configs:
    for d in depths:
        e = B.Engine({**cfg, 'depth': d})
        for p in canon:
            r = e.search_depth(p['fen'])
            rows[cfg['name']][f"{p['name']}@d{d}"] = {
                'uci': r.get('uci'), 'scoreCp': r.get('scoreCp'), 'mate': r.get('mate'),
                'nodes': r.get('nodes'), 'qNodes': r.get('qNodes'),
                'ttHits': r.get('ttHits'), 'timeMs': r.get('timeMs'),
                'pv': ' '.join(r.get('pv', [])[:6]),
            }
        e.close()

names = [c['name'] for c in configs]
mismatch = []
print(f"{'position':28s} " + '  '.join(f'{n:>34s}' for n in names))
for key in rows[names[0]]:
    line = f'{key:28s} '
    vals = []
    for n in names:
        v = rows[n][key]
        vals.append(v)
        line += f"  {str(v['uci']):>7s} cp{str(v['scoreCp']):>6s} n{v['nodes']:>9d} {v['timeMs']:>5d}ms"
    print(line)
    if len(vals) == 2 and (vals[0]['uci'] != vals[1]['uci'] or vals[0]['nodes'] != vals[1]['nodes']):
        mismatch.append(key)

if len(names) == 2:
    label = 'IDENTICAL' if not mismatch else f'{len(mismatch)} DIFFERENCES: {mismatch[:8]}'
    print(f'\nGATE 1 (speed-patch criterion): {label}')
    print('(differences are expected for pruning/search patches -> run Gate 3)')

B.write_result('gate1-parity', {
    'provenance': [B.provenance(c) for c in configs],
    'depths': depths, 'rows': rows, 'mismatches': mismatch,
})
