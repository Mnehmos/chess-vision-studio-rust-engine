#!/usr/bin/env python3
import json
import os
import random
import subprocess
import argparse
from pathlib import Path
import chess

# Default Paths to executables and weights
SF_EXE = os.environ.get(
    "CVS_SF_EXE",
    "f:/tools/stockfish/stockfish/stockfish-windows-x86-64-avx2.exe",
)
CVS_EXE = os.environ.get("CVS_RUST_UCI", "target/release/uci.exe")
BOOK_PATH = os.environ.get("CVS_OPENING_BOOK", "F:/tools/openings.epd")
OUT_PATH = os.environ.get(
    "CVS_DISAGREEMENT_OUT", "F:/tools/gen9-disagreement-seeds.jsonl"
)

# Game settings
SEARCH_TIME_MS = 100
SF_DEPTH = 15
SF_LABEL_DEPTH = 24
BRANCH_DEPTH_PLIES = 20

class EngineProcess:
    def __init__(self, cmd, args=None):
        self.cmd = cmd
        self.args = list(args or [])
        full_args = [cmd] + self.args
        self.p = subprocess.Popen(full_args, stdin=subprocess.PIPE, stdout=subprocess.PIPE, text=True, bufsize=1)
        self._send('uci')
        self._send('isready')
        self._wait_for('readyok')

    def _send(self, line):
        self.p.stdin.write(line + '\n')
        self.p.stdin.flush()

    def _wait_for(self, target):
        while True:
            line = self.p.stdout.readline()
            if not line:
                raise RuntimeError(f"Engine process {self.cmd} exited prematurely")
            if target in line:
                return line

    def get_move(self, fen, movetime_ms=100, depth=None):
        self._send(f'position fen {fen}')
        if depth:
            self._send(f'go depth {depth}')
        else:
            self._send(f'go movetime {movetime_ms}')
        
        bestmove = None
        pv = []
        info_lines = []
        while True:
            line = self.p.stdout.readline()
            if not line:
                break
            if line.startswith('info'):
                info_lines.append(line)
                if ' pv ' in line:
                    parts = line.split()
                    try:
                        pv_idx = parts.index('pv')
                        pv = parts[pv_idx + 1:]
                    except ValueError:
                        pass
            if line.startswith('bestmove'):
                bestmove = line.split()[1]
                break
        return bestmove, pv, info_lines

    def evaluate_move(self, fen, move_uci, depth=12):
        self._send(f'position fen {fen} moves {move_uci}')
        self._send(f'go depth {depth}')
        score = 0
        while True:
            line = self.p.stdout.readline()
            if not line:
                break
            if ' score cp ' in line:
                t = line.split(' score cp ')
                score = int(t[1].split()[0])
            elif ' score mate ' in line:
                t = line.split(' score mate ')
                mate = int(t[1].split()[0])
                score = 10000 if mate > 0 else -10000
            if line.startswith('bestmove'):
                break
        return score

    def close(self):
        try:
            self._send('quit')
        except Exception:
            pass
        try:
            self.p.wait(timeout=2)
        except subprocess.TimeoutExpired:
            self.p.kill()
            self.p.wait(timeout=2)

def load_book(path):
    if not os.path.exists(path):
        return ["rnbqkbnr/pppppppp/8/8/8/8/PPPPPPPP/RNBQKBNR w KQkq - 0 1"]
    with open(path, encoding='utf8') as f:
        return [line.split(';', 1)[0].strip() for line in f if line.strip()]

def play_rollout(
    fen,
    moves_prefix,
    steps,
    cvs,
    sf,
    record_callback,
    sf_depth=SF_DEPTH,
    cvs_movetime_ms=SEARCH_TIME_MS,
):
    board = chess.Board(fen)
    for mv in moves_prefix:
        move = chess.Move.from_uci(mv)
        if move not in board.legal_moves:
            return
        board.push(move)

    for step in range(steps):
        if board.is_game_over():
            break
        
        current_fen = board.fen()
        # Stockfish plays White, CVS plays Black (symmetric playout)
        if board.turn == chess.WHITE:
            mv_uci, _, _ = sf.get_move(current_fen, depth=sf_depth)
        else:
            mv_uci, _, _ = cvs.get_move(current_fen, movetime_ms=cvs_movetime_ms)

        if not mv_uci or mv_uci == "(none)":
            break
        if not board.is_check():
            record_callback(current_fen)

        move = chess.Move.from_uci(mv_uci)
        if move not in board.legal_moves:
            break
        board.push(move)

def record_position(fen, sf, out_file, collected_fens, depth=SF_LABEL_DEPTH):
    if fen in collected_fens:
        return
    collected_fens.add(fen)
    
    _, _, info = sf.get_move(fen, depth=depth)
    
    score = 0
    mate = None
    for line in reversed(info):
        if ' score cp ' in line:
            score = int(line.split(' score cp ')[1].split()[0])
            break
        elif ' score mate ' in line:
            mate = int(line.split(' score mate ')[1].split()[0])
            score = 32000 if mate > 0 else -32000
            break

    board = chess.Board(fen)
    white_cp = score if board.turn == chess.WHITE else -score
    
    row = {'fen': fen, 'cp': white_cp, 'res': 0.5}
    out_file.write(json.dumps(row) + '\n')
    out_file.flush()
    print(f"    [Recorded] FEN: {fen} | cp: {white_cp}", flush=True)

def main():
    parser = argparse.ArgumentParser()
    parser.add_argument('--games', type=int, default=50, help="Number of games to run")
    parser.add_argument('--movetime', type=int, default=SEARCH_TIME_MS, help="Move search time limit in ms")
    parser.add_argument('--sf-depth', type=int, default=SF_DEPTH, help="Stockfish search depth in games/playouts")
    parser.add_argument('--sf-label-depth', type=int, default=SF_LABEL_DEPTH, help="Stockfish evaluation label depth")
    parser.add_argument('--branch-plies', type=int, default=BRANCH_DEPTH_PLIES, help="Number of plies to play out on deviation")
    parser.add_argument('--stockfish', default=SF_EXE, help="Stockfish executable")
    parser.add_argument('--cvs', default=CVS_EXE, help="CVS UCI executable")
    parser.add_argument('--book', default=BOOK_PATH, help="EPD/FEN opening file")
    parser.add_argument('--output', default=OUT_PATH, help="Output JSONL path")
    args = parser.parse_args()

    print("Initializing engine processes...", flush=True)
    print("Spawning Model A (Gen9-Raw)...", flush=True)
    model_a = EngineProcess(args.cvs, args=['--nnue', 'target-cvs/matrix-raw.json', '--allow-unverified-net'])
    print("Spawning Model B (Quiet-Hybrid-A)...", flush=True)
    model_b = EngineProcess(args.cvs, args=['--nnue', 'target-cvs/matrix-raw.json', '--helper-nnue', 'target-cvs/matrix-residual.json', '--allow-unverified-net'])
    print("Spawning Stockfish...", flush=True)
    sf = EngineProcess(args.stockfish)

    book = load_book(args.book)
    print(f"Loaded {len(book)} opening positions from book.", flush=True)

    collected_fens = set()
    if os.path.exists(args.output):
        print(f"Loading existing collected FENs from {args.output}...", flush=True)
        with open(args.output, encoding='utf8') as f:
            for line in f:
                if line.strip():
                    try:
                        collected_fens.add(json.loads(line)['fen'])
                    except Exception:
                        pass
        print(f"Loaded {len(collected_fens)} existing unique FENs.", flush=True)

    Path(args.output).parent.mkdir(parents=True, exist_ok=True)
    out_file = open(args.output, 'a', encoding='utf8')

    try:
        for game_idx in range(args.games):
            start_fen = random.choice(book)
            board = chess.Board(start_fen)
            print(f"\n--- Game {game_idx+1}/{args.games} starting from {start_fen} ---", flush=True)
            
            # Alternate White and Black between Model A and Model B
            model_white = model_a if game_idx % 2 == 0 else model_b
            model_black = model_b if game_idx % 2 == 0 else model_a
            
            white_name = "Model A (Raw)" if model_white == model_a else "Model B (Hybrid A)"
            black_name = "Model A (Raw)" if model_black == model_a else "Model B (Hybrid A)"
            print(f"White: {white_name} | Black: {black_name}", flush=True)

            expected_replies = {} # move_index -> expected opponent reply move uci

            while not board.is_game_over() and len(board.move_stack) < 100:
                current_fen = board.fen()
                move_idx = len(board.move_stack)
                
                active_model = model_white if board.turn == chess.WHITE else model_black
                inactive_model = model_black if board.turn == chess.WHITE else model_white
                
                active_name = "Model A (Raw)" if active_model == model_a else "Model B (Hybrid A)"
                inactive_name = "Model A (Raw)" if inactive_model == model_a else "Model B (Hybrid A)"

                # 1. Active model searches to choose its move and predicts PV
                best_move, active_pv, _ = active_model.get_move(current_fen, movetime_ms=args.movetime)
                
                # 2. Check if this move deviates from the inactive model's predicted PV
                expected_move = expected_replies.get(move_idx)
                if expected_move is not None:
                    if best_move != expected_move:
                        print(f"[*] DISAGREEMENT at ply {move_idx}!", flush=True)
                        print(f"    {inactive_name} expected {expected_move}", flush=True)
                        print(f"    {active_name} played {best_move}", flush=True)
                        
                        # Branch Expected
                        print("    Running Branch Expected...", flush=True)
                        play_rollout(
                            current_fen,
                            [expected_move],
                            args.branch_plies,
                            model_b,
                            sf,
                            lambda f: record_position(
                                f, sf, out_file, collected_fens, args.sf_label_depth
                            ),
                            args.sf_depth,
                            args.movetime,
                        )
                        
                        # Branch Actual
                        print("    Running Branch Actual...", flush=True)
                        play_rollout(
                            current_fen,
                            [best_move],
                            args.branch_plies,
                            model_b,
                            sf,
                            lambda f: record_position(
                                f, sf, out_file, collected_fens, args.sf_label_depth
                            ),
                            args.sf_depth,
                            args.movetime,
                        )

                # 3. Store expected opponent reply for next ply
                if len(active_pv) >= 2:
                    expected_replies[move_idx + 1] = active_pv[1]
                elif (move_idx + 1) in expected_replies:
                    del expected_replies[move_idx + 1]

                # 4. Push move to main board
                if not best_move or best_move == "(none)":
                    break
                move = chess.Move.from_uci(best_move)
                if move not in board.legal_moves:
                    print(f"Illegal engine move {best_move}; ending game.", flush=True)
                    break
                board.push(move)

            print(f"Game {game_idx+1} finished. FENs collected: {len(collected_fens)}", flush=True)
    except KeyboardInterrupt:
        print("\nInterrupted by user. Exiting cleanly...", flush=True)
    finally:
        model_a.close()
        model_b.close()
        sf.close()
        out_file.close()
        print(f"Finished. Total FENs saved: {len(collected_fens)}", flush=True)

if __name__ == '__main__':
    main()
