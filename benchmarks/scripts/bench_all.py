import sys
import statistics
import subprocess
import benchlib as B

depth = 30
movetime = 500

class StockfishEngine:
    def __init__(self, name='Stockfish-Ref', exe=B.STOCKFISH):
        self.name = name
        self.exe = exe
        self.p = subprocess.Popen([self.exe], stdin=subprocess.PIPE,
                                  stdout=subprocess.PIPE, text=True, bufsize=1)
        self.p.stdin.write('uci\nisready\n')
        self.p.stdin.flush()
        while 'readyok' not in self.p.stdout.readline():
            pass

    def search_time(self, fen, ms):
        self.p.stdin.write(f'position fen {fen}\ngo movetime {ms}\n')
        self.p.stdin.flush()
        while True:
            ln = self.p.stdout.readline()
            if ln.startswith('bestmove'):
                bm = ln.split()[1]
                return {'uci': bm}

    def close(self):
        self.p.kill()

suite = B.load_suite('suite-fresh-100')
sf_depth = 15
sf = B.Stockfish(depth=sf_depth)
fens, orc, danger = suite['fens'], suite['oracle'], suite['danger'] or [False] * len(suite['fens'])
n = len(fens)

configs = [
    B.engine_cfg('Gen7-Baseline', exe='f:/tools/cvs-baselines/analyze-gen7-acc-futility.exe', net='f:/tools/cvs-baselines/raw-nnue-h256-sf-d12-v3.json', depth=depth),
    B.engine_cfg('Gen8-Champion', exe='f:/tools/cvs-baselines/analyze-gen8v2-champion.exe', net='f:/tools/cvs-baselines/raw-nnue-h256-sf-d12-v3.json', depth=depth),
    B.engine_cfg('Gen9-Raw', exe='target/release/analyze.exe', net='target-cvs/matrix-raw.json', depth=depth),
    B.engine_cfg('Gen9-Flat', exe='target/release/analyze.exe', net='target-cvs/matrix-flat.json', depth=depth),
    B.engine_cfg('Gen9-Residual', exe='target/release/analyze.exe', net='target-cvs/matrix-residual.json', depth=depth, extra=['--allow-unverified-net']),
    {'name': 'Stockfish-Ref', 'is_stockfish': True}
]

print(f"# Gate 3 cp-loss — {suite['name']} (hash {suite['hash']}), engine movetime {movetime}ms, SF-d{sf_depth} child evals")
print(f"{'config':>15s} {'match%':>7s} {'avgCP':>7s} {'medCP':>6s} {'p90':>5s} {'p95':>5s} {'bl100':>6s} {'bl200':>6s} {'danger':>7s} {'quiet':>7s}")

for cfg in configs:
    try:
        if cfg.get('is_stockfish'):
            e = StockfishEngine()
        else:
            e = B.Engine(cfg)
        moves = []
        for f in fens:
            r = e.search_time(f, int(movetime))
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
        fmt = lambda v: f'{v:>7.1f}' if v is not None else '      -'
        print(f"{cfg['name']:>15s} {stats['match_pct']:>6.1f}% {stats['avg']:>7.1f} {stats['median']:>6.0f} {stats['p90']:>5.0f} {stats['p95']:>5.0f} {stats['bl100_pct']:>5.1f}% {stats['bl200_pct']:>5.1f}% {fmt(stats['danger_avg'])} {fmt(stats['quiet_avg'])}", flush=True)
    except Exception as e:
        print(f"{cfg['name']:>15s}  FAILED: {str(e)}")

sf.close()
