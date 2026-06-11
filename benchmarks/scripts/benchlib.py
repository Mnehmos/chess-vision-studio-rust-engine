# Shared library for the snapshot benchmark suite (see benchmarks/README.md).
#
# Provides: the engine serve-protocol client, the Stockfish scorer, suite
# loading/hashing, and the Gate-0 provenance record that every result file
# must embed. No engine-strength logic lives here.
import hashlib
import json
import os
import platform
import subprocess
import sys
from datetime import datetime, timezone

REPO = os.path.normpath(os.path.join(os.path.dirname(__file__), '..', '..'))
SUITES = os.path.normpath(os.path.join(os.path.dirname(__file__), '..', 'suites'))
RESULTS = os.path.normpath(os.path.join(os.path.dirname(__file__), '..', 'results'))

# ---- the frozen baseline (snapshot/gen7-acc-futility-2026-06-11) ----
BASELINE = {
    'name': 'snapshot/gen7-acc-futility-2026-06-11',
    'commit': 'f07caae',
    'serve_exe': 'f:/tools/cvs-baselines/analyze-gen7-acc-futility.exe',
    'uci_exe': 'f:/tools/cvs-baselines/uci-gen7-acc-futility.exe',
    'net': 'f:/tools/cvs-baselines/raw-nnue-h256-sf-d12-v3.json',
    'base_weights': 'f:/Github/chess-vision-studio/arena/out/value-weights-mixed.json',
    'rung2_weights': 'f:/Github/chess-vision-studio/arena/out/rung2-weights-mixed.json',
    'futility': True,  # accepted-with-note; ALWAYS recorded per run
}
STOCKFISH = 'f:/tools/stockfish/stockfish/stockfish-windows-x86-64-avx2.exe'


def sha256(path, n=16):
    h = hashlib.sha256()
    with open(path, 'rb') as f:
        for chunk in iter(lambda: f.read(1 << 20), b''):
            h.update(chunk)
    return h.hexdigest()[:n]


def git(args):
    try:
        return subprocess.check_output(['git', '-C', REPO] + args, text=True).strip()
    except Exception:
        return '(unavailable)'


def provenance(cfg):
    """Gate-0 identity record. cfg = engine config dict (see engine_cfg)."""
    return {
        'date': datetime.now(timezone.utc).isoformat(timespec='seconds'),
        'git_commit': git(['rev-parse', '--short', 'HEAD']),
        'git_describe': git(['describe', '--tags', '--always', '--dirty']),
        'machine': platform.node(),
        'engine_exe': cfg['exe'],
        'engine_sha': sha256(cfg['exe']),
        'net': cfg.get('net'),
        'net_sha': sha256(cfg['net']) if cfg.get('net') else None,
        'futility': bool(cfg.get('futility')),
        'extra_flags': cfg.get('extra', []),
        'threads': cfg.get('threads', 1),
        'depth': cfg.get('depth'),
        'baseline_ref': BASELINE['name'],
    }


def engine_cfg(name='baseline', exe=None, net=None, futility=None, extra=None,
               threads=1, depth=30):
    """Build an engine config. Defaults to the frozen snapshot."""
    return {
        'name': name,
        'exe': exe or BASELINE['serve_exe'],
        'net': net or BASELINE['net'],
        'futility': BASELINE['futility'] if futility is None else futility,
        'extra': extra or [],
        'threads': threads,
        'depth': depth,
    }


class Engine:
    """analyze --serve client: fixed-depth (plain fen) and movetime (JSON go)."""

    def __init__(self, cfg):
        self.cfg = cfg
        args = [cfg['exe'], '--serve', '--depth', str(cfg['depth']),
                '--threads', str(cfg['threads']),
                '--base', BASELINE['base_weights'],
                '--rung2', BASELINE['rung2_weights']]
        if cfg.get('net'):
            args += ['--nnue', cfg['net']]
        if cfg.get('futility'):
            args.append('--futility')
        args += cfg.get('extra', [])
        self.p = subprocess.Popen(args, stdin=subprocess.PIPE,
                                  stdout=subprocess.PIPE, text=True, bufsize=1)

    def _ask(self, line):
        self.p.stdin.write(line + '\n')
        self.p.stdin.flush()
        return json.loads(self.p.stdout.readline())

    def search_depth(self, fen):
        """Fixed-depth search (the process's --depth)."""
        return self._ask(fen)

    def search_time(self, fen, ms):
        """Movetime search (depth caps at the process's --depth)."""
        return self._ask(json.dumps({'cmd': 'go', 'budgetMs': int(ms), 'fen': fen}))

    def close(self):
        try:
            self.p.stdin.write('quit\n')
            self.p.stdin.flush()
        except Exception:
            pass
        self.p.kill()


class Stockfish:
    """Fixed-depth scorer (Gate 3): mover-POV cp of a child position."""

    def __init__(self, depth=12):
        self.depth = depth
        self.p = subprocess.Popen([STOCKFISH], stdin=subprocess.PIPE,
                                  stdout=subprocess.PIPE, text=True, bufsize=1)
        self.p.stdin.write('uci\nisready\n')
        self.p.stdin.flush()
        while 'readyok' not in self.p.stdout.readline():
            pass
        self.cache = {}

    def go(self, fen, depth=None):
        self.p.stdin.write(f'position fen {fen}\ngo depth {depth or self.depth}\n')
        self.p.stdin.flush()
        sc, bm = 0, None
        while True:
            ln = self.p.stdout.readline()
            if ' score cp ' in ln:
                sc = int(ln.split(' score cp ')[1].split()[0])
            elif ' score mate ' in ln:
                sc = 10000 if int(ln.split(' score mate ')[1].split()[0]) > 0 else -10000
            elif ln.startswith('bestmove'):
                return sc, ln.split()[1]

    def child_cp(self, fen, uci):
        """SF eval of child(fen, uci) from the MOVER's POV; None if illegal."""
        import chess
        key = (fen, uci, self.depth)
        if key in self.cache:
            return self.cache[key]
        b = chess.Board(fen)
        try:
            b.push(chess.Move.from_uci(uci))
        except Exception:
            self.cache[key] = None
            return None
        if b.is_checkmate():
            r = 10000
        elif b.is_stalemate() or b.is_insufficient_material():
            r = 0
        else:
            sc, _ = self.go(b.fen())
            r = -sc
        self.cache[key] = r
        return r

    def close(self):
        self.p.kill()


def load_suite(name):
    """Load a suite by stem (e.g. 'suite-fresh-100'). Returns dict with fens,
    danger flags, saved oracle moves, and the suite hash (provenance)."""
    txt = os.path.join(SUITES, name + '.txt')
    fens = [l.strip() for l in open(txt) if l.strip()]
    out = {'name': name, 'fens': fens, 'hash': sha256(txt)}
    dj = os.path.join(SUITES, name + '.danger.json')
    mj = os.path.join(SUITES, name + '.moves.json')
    out['danger'] = json.load(open(dj)) if os.path.exists(dj) else None
    out['oracle'] = json.load(open(mj))['ORACLE'] if os.path.exists(mj) else None
    return out


def write_result(stem, payload):
    os.makedirs(RESULTS, exist_ok=True)
    ts = datetime.now().strftime('%Y%m%d-%H%M%S')
    path = os.path.join(RESULTS, f'{ts}-{stem}.json')
    json.dump(payload, open(path, 'w'), indent=1)
    print(f'[result] {path}', file=sys.stderr)
    return path
