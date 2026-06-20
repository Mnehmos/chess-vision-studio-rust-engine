# Shared library for the snapshot benchmark suite (see benchmarks/README.md).
#
# Provides: the engine serve-protocol client, the Stockfish scorer, suite
# loading/hashing, and the Gate-0 provenance record that every result file
# must embed. No engine-strength logic lives here.
import hashlib
import json
import os
import platform
import re
import subprocess
import sys
import ctypes
from datetime import datetime, timezone

REPO = os.path.normpath(os.path.join(os.path.dirname(__file__), '..', '..'))
SUITES = os.path.normpath(os.path.join(os.path.dirname(__file__), '..', 'suites'))
RESULTS = os.path.normpath(os.path.join(os.path.dirname(__file__), '..', 'results'))
ENGINE_REGISTRY = os.path.normpath(
    os.path.join(os.path.dirname(__file__), '..', 'engines.json')
)

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
DEFAULT_STOCKFISH_REVIEW_DEPTH = 24


def canonical_hash(value, n=16):
    return hashlib.sha256(
        json.dumps(value, sort_keys=True, separators=(',', ':')).encode('utf8')
    ).hexdigest()[:n]


def sha256(path, n=16):
    h = hashlib.sha256()
    with open(path, 'rb') as f:
        for chunk in iter(lambda: f.read(1 << 20), b''):
            h.update(chunk)
    return h.hexdigest()[:n]


def _expand_env_token(match):
    key = match.group(1)
    fallback = match.group(2) or ''
    return os.environ.get(key, fallback)


def resolve_path(path):
    """Resolve registry paths with ${NAME:-fallback} support."""
    if not path:
        return None
    value = re.sub(r'\$\{([A-Za-z_][A-Za-z0-9_]*)(?::-([^}]*))?\}',
                   _expand_env_token, path)
    value = os.path.expanduser(os.path.expandvars(value))
    if not os.path.isabs(value):
        value = os.path.join(REPO, value)
    return os.path.normpath(value)


def load_engine_registry(path=None):
    registry_path = resolve_path(path) if path else ENGINE_REGISTRY
    with open(registry_path, encoding='utf8') as handle:
        registry = json.load(handle)
    registry['_path'] = registry_path
    return registry


def registered_engine(engine_id, registry=None, depth=30, threads=None):
    registry = registry or load_engine_registry()
    row = next((item for item in registry['engines'] if item['id'] == engine_id), None)
    if row is None:
        raise KeyError(f'unknown engine id: {engine_id}')
    profile_id = row['searchProfile']
    profile = registry['searchProfiles'][profile_id]
    extra = list(profile.get('args', []))
    helper = resolve_path(row.get('helperNet'))
    if helper:
        extra += ['--helper-nnue', helper]
    return {
        'id': row['id'],
        'name': row['displayName'],
        'generation': row['generation'],
        'status': row['status'],
        'architecture': row['architecture'],
        'search_profile': profile_id,
        'search_profile_sha': canonical_hash(profile),
        'expected_search_options': profile.get('effectiveOptions', {}),
        'policy': row.get('policy', {}),
        'policy_sha': canonical_hash(row.get('policy', {})),
        'exe': resolve_path(row['serveExe']),
        'uci_exe': resolve_path(row.get('uciExe')),
        'net': resolve_path(row.get('mainNet')),
        'helper_net': helper,
        'base_weights': resolve_path(row.get('baseWeights') or BASELINE['base_weights']),
        'rung2_weights': resolve_path(row.get('rung2Weights') or BASELINE['rung2_weights']),
        'futility': bool(row.get('legacyFutilityFlag', False)),
        'extra': extra,
        'threads': threads or row.get('defaultThreads', 1),
        'depth': depth,
        'notes': row.get('notes', ''),
    }


def git(args):
    try:
        return subprocess.check_output(['git', '-C', REPO] + args, text=True).strip()
    except Exception:
        return '(unavailable)'


def _command_version(command):
    try:
        return subprocess.check_output(command, text=True, stderr=subprocess.STDOUT).strip()
    except Exception:
        return '(unavailable)'


def _memory_bytes():
    if os.name != 'nt':
        return None

    class MemoryStatus(ctypes.Structure):
        _fields_ = [
            ('length', ctypes.c_ulong),
            ('memory_load', ctypes.c_ulong),
            ('total_phys', ctypes.c_ulonglong),
            ('avail_phys', ctypes.c_ulonglong),
            ('total_page_file', ctypes.c_ulonglong),
            ('avail_page_file', ctypes.c_ulonglong),
            ('total_virtual', ctypes.c_ulonglong),
            ('avail_virtual', ctypes.c_ulonglong),
            ('avail_extended_virtual', ctypes.c_ulonglong),
        ]

    status = MemoryStatus()
    status.length = ctypes.sizeof(MemoryStatus)
    return status.total_phys if ctypes.windll.kernel32.GlobalMemoryStatusEx(
        ctypes.byref(status)
    ) else None


def machine_info():
    return {
        'hostname': platform.node(),
        'os': platform.platform(),
        'architecture': platform.machine(),
        'processor': os.environ.get('PROCESSOR_IDENTIFIER') or platform.processor(),
        'logical_cpus': os.cpu_count(),
        'memory_bytes': _memory_bytes(),
        'python': sys.version.split()[0],
        'rustc': _command_version(['rustc', '--version']),
        'cargo': _command_version(['cargo', '--version']),
    }


def model_metadata(path):
    if not path or not os.path.exists(path):
        return None
    try:
        with open(path, encoding='utf8') as handle:
            value = json.load(handle)
    except Exception:
        return {'parse_error': True}
    keys = (
        'modelKind', 'arch', 'registryVersion', 'registryHash', 'rows',
        'epochs', 'hidden', 'psHidden', 'cvsHidden', 'cvsDim',
        'featureCount', 'trainingCommit', 'datasetManifestHash',
    )
    return {key: value.get(key) for key in keys if key in value}


def provenance(cfg):
    """Gate-0 identity record. cfg = engine config dict (see engine_cfg)."""
    return {
        'date': datetime.now(timezone.utc).isoformat(timespec='seconds'),
        'git_commit': git(['rev-parse', '--short', 'HEAD']),
        'git_describe': git(['describe', '--tags', '--always', '--dirty']),
        'machine': machine_info(),
        'engine_id': cfg.get('id'),
        'generation': cfg.get('generation'),
        'status': cfg.get('status'),
        'architecture': cfg.get('architecture'),
        'search_profile': cfg.get('search_profile'),
        'search_profile_sha': cfg.get('search_profile_sha'),
        'policy': cfg.get('policy', {}),
        'policy_sha': cfg.get('policy_sha'),
        'engine_exe': cfg['exe'],
        'engine_sha': sha256(cfg['exe']) if os.path.exists(cfg['exe']) else None,
        'uci_exe': cfg.get('uci_exe'),
        'uci_sha': sha256(cfg['uci_exe']) if cfg.get('uci_exe') and os.path.exists(cfg['uci_exe']) else None,
        'net': cfg.get('net'),
        'net_sha': sha256(cfg['net']) if cfg.get('net') and os.path.exists(cfg['net']) else None,
        'net_metadata': model_metadata(cfg.get('net')),
        'helper_net': cfg.get('helper_net'),
        'helper_net_sha': sha256(cfg['helper_net']) if cfg.get('helper_net') and os.path.exists(cfg['helper_net']) else None,
        'helper_net_metadata': model_metadata(cfg.get('helper_net')),
        'base_weights': cfg.get('base_weights') or BASELINE['base_weights'],
        'base_weights_sha': sha256(cfg.get('base_weights') or BASELINE['base_weights'])
        if os.path.exists(cfg.get('base_weights') or BASELINE['base_weights']) else None,
        'rung2_weights': cfg.get('rung2_weights') or BASELINE['rung2_weights'],
        'rung2_weights_sha': sha256(cfg.get('rung2_weights') or BASELINE['rung2_weights'])
        if os.path.exists(cfg.get('rung2_weights') or BASELINE['rung2_weights']) else None,
        'legacy_futility_cli_flag': bool(cfg.get('futility')),
        'expected_search_options': cfg.get('expected_search_options', {}),
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
                '--base', cfg.get('base_weights') or BASELINE['base_weights'],
                '--rung2', cfg.get('rung2_weights') or BASELINE['rung2_weights']]
        if cfg.get('net'):
            args += ['--nnue', cfg['net']]
        if cfg.get('futility'):
            args.append('--futility')
        args += cfg.get('extra', [])
        self.command = args
        self.p = subprocess.Popen(args, stdin=subprocess.PIPE,
                                  stdout=subprocess.PIPE, stderr=subprocess.PIPE,
                                  text=True, bufsize=1)

    def _ask(self, line):
        self.p.stdin.write(line + '\n')
        self.p.stdin.flush()
        reply = self.p.stdout.readline()
        if not reply:
            stderr = self.p.stderr.read()
            raise RuntimeError(
                f"engine exited before replying ({self.p.returncode}): {stderr.strip()}"
            )
        return json.loads(reply)

    def search_depth(self, fen):
        """Fixed-depth search (the process's --depth)."""
        return self._ask(fen)

    def search_time(self, fen, ms, forced_move=None):
        """Movetime search (depth caps at the process's --depth)."""
        request = {'cmd': 'go', 'budgetMs': int(ms), 'fen': fen}
        if forced_move:
            request['forcedMoveUci'] = forced_move
        return self._ask(json.dumps(request))

    def close(self):
        try:
            self.p.stdin.write('quit\n')
            self.p.stdin.flush()
        except Exception:
            pass
        self.p.kill()


class Stockfish:
    """Fixed-depth scorer (Gate 3): mover-POV cp of a child position."""

    def __init__(self, depth=DEFAULT_STOCKFISH_REVIEW_DEPTH):
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
