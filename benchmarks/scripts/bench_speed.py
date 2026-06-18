import json
import statistics
import benchlib as B

depth = 30
times = [100, 1000]

canon = json.load(open(f'{B.SUITES}/canonical.json'))
positions = [p for p in canon['positions'] if p['name'] in canon['ladder_subset']]

configs = [
    B.engine_cfg('Gen7-Baseline', exe='f:/tools/cvs-baselines/analyze-gen7-acc-futility.exe', net='f:/tools/cvs-baselines/raw-nnue-h256-sf-d12-v3.json', depth=depth),
    B.engine_cfg('Gen8-Champion', exe='f:/tools/cvs-baselines/analyze-gen8v2-champion.exe', net='f:/tools/cvs-baselines/raw-nnue-h256-sf-d12-v3.json', depth=depth),
    B.engine_cfg('Gen9-Raw', exe='target/release/analyze.exe', net='target-cvs/matrix-raw.json', depth=depth),
    B.engine_cfg('Gen9-Flat', exe='target/release/analyze.exe', net='target-cvs/matrix-flat.json', depth=depth),
    B.engine_cfg('Gen9-Residual', exe='target/release/analyze.exe', net='target-cvs/matrix-residual.json', depth=depth),
]

print(f"# Speed/NPS Benchmark — 1 thread, {len(positions)} canonical positions")
print(f"{'config':>15s} {'NPS(100ms)':>12s} {'Depth(100ms)':>12s}   {'NPS(1000ms)':>12s} {'Depth(1000ms)':>12s}")

for cfg in configs:
    try:
        e = B.Engine(cfg)
        results = {}
        for ms in times:
            ds, ns = [], []
            for p in positions:
                r = e.search_time(p['fen'], ms)
                ds.append(r.get('depth', 0))
                el = max(1, r.get('timeMs', ms))
                ns.append(r.get('nodes', 0) / el / 1000)
            results[ms] = {
                'avg_depth': statistics.mean(ds),
                'avg_mnps': statistics.mean(ns)
            }
        e.close()
        
        print(f"{cfg['name']:>15s} "
              f"{results[100]['avg_mnps']:>10.2f}M {results[100]['avg_depth']:>11.1f}   "
              f"{results[1000]['avg_mnps']:>10.2f}M {results[1000]['avg_depth']:>11.1f}", flush=True)
    except Exception as e_err:
        print(f"{cfg['name']:>15s}  FAILED: {str(e_err)}")
