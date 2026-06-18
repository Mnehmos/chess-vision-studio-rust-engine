#!/usr/bin/env python3
import json
import os
import random
import subprocess
import sys
import time
import chess

# Paths to executables and weights
SF_EXE = 'f:/tools/stockfish/stockfish/stockfish-windows-x86-64-avx2.exe'
CVS_EXE = 'target/release/uci.exe'
BASE_W = 'f:/Github/chess-vision-studio/arena/out/value-weights-mixed.json'
RUNG2_W = 'f:/Github/chess-vision-studio/arena/out/rung2-weights-mixed.json'
MODEL_NET = 'f:/Github/chess-vision-studio/arena/out/gen9-raw-h256-d20.json'
BOOK_PATH = 'F:/tools/openings.epd'
OUT_PATH = 'F:/tools/gen9-disagreement-seeds.jsonl'

# Game settings
SEARCH_TIME_MS = 100
SF_DEPTH = 15
SF_LABEL_DEPTH = 20
BRANCH_DEPTH_PLIES = 20
MISTAKE_THRESHOLD_CP = 50

class EngineProcess:
    def __init__(self, cmd, is_cvs=False):
        self.is_cvs = is_cvs
        args = [cmd]
        if is_cvs:
            args += ['--base', BASE_W, '--rung2', RUNG2_W, '--nnue', MODEL_NET, '--futility']
        self.p = subprocess.Popen(args, stdin=subprocess.PIPE, stdout=subprocess.PIPE, text=True, bufsize=1)
        self._send('uci')
        if is_cvs:
            self._send('isready')
        else:
            self._send('setoption name Threads value 1')
            self._send('setoption name Hash value 256')
            self._send('isready')
        self._wait_for('readyok')

    def _send(self, line):
        self.p.stdin.write(line + '\n')
        self.p.stdin.flush()

    def _wait_for(self, target):
        while True:
            line = self.p.stdout.readline()
            if not line:
                raise RuntimeError("Engine process exited prematurely")
            if target in line:
                return line

    def get_move(self, fen, movetime_ms=100, depth=None):
        self._send(f'position fen {fen}')
        if depth:
            self._send(f'go depth {depth}')
        else:
            self._send(f'go movetime {movetime_ms}')
        
        bestmove = None
        info_lines = []
        while True:
            line = self.p.stdout.readline()
            if not line:
                break
            if line.startswith('info'):
                info_lines.append(line)
            if line.startswith('bestmove'):
                bestmove = line.split()[1]
                break
        return bestmove, info_lines

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
        self.p.kill()

def load_book():
    if not os.path.exists(BOOK_PATH):
        return ["rnbqkbnr/pppppppp/8/8/8/8/PPPPPPPP/RNBQKBNR w KQkq - 0 1"]
    with open(BOOK_PATH, encoding='utf8') as f:
        return [line.strip() for line in f if line.strip()]

def play_rollout(fen, moves_prefix, steps, cvs, sf, record_callback):
    board = chess.Board(fen)
    for mv in moves_prefix:
        board.push(chess.Move.from_uci(mv))

    for step in range(steps):
        if board.is_game_over():
            break
        
        current_fen = board.fen()
        if board.turn == chess.WHITE:
            mv_uci, _ = sf.get_move(current_fen, depth=SF_DEPTH)
        else:
            mv_uci, _ = cvs.get_move(current_fen, movetime_ms=SEARCH_TIME_MS)

        if not board.is_check():
            record_callback(current_fen)

        board.push(chess.Move.from_uci(mv_uci))

def main():
    print("Initializing CVS and Stockfish processes...")
    cvs = EngineProcess(CVS_EXE, is_cvs=True)
    sf = EngineProcess(SF_EXE, is_cvs=False)

    book = load_book()
    print(f"Loaded {len(book)} opening positions from book.")

    collected_fens = set()
    
    # Load already collected FENs from file to avoid duplicates
    if os.path.exists(OUT_PATH):
        print(f"Loading existing collected FENs from {OUT_PATH}...")
        with open(OUT_PATH, encoding='utf8') as f:
            for line in f:
                if line.strip():
                    try:
                        collected_fens.add(json.loads(line)['fen'])
                    except Exception:
                        pass
        print(f"Loaded {len(collected_fens)} existing unique FENs.")

    out_file = open(OUT_PATH, 'a', encoding='utf8')

    def record_position(fen):
        if fen in collected_fens:
            return
        collected_fens.add(fen)
        bestmove, info = sf.get_move(fen, depth=SF_LABEL_DEPTH)
        
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
        print(f"  [Recorded] FEN: {fen} | cp: {white_cp}")

    games_to_play = 50
    print(f"Starting self-play loop for {games_to_play} games. Writing to {OUT_PATH}")

    try:
        for game_idx in range(games_to_play):
            start_fen = random.choice(book)
            board = chess.Board(start_fen)
            print(f"\n--- Game {game_idx+1}/{games_to_play} starting from {start_fen} ---")

            while not board.is_game_over() and len(board.move_stack) < 80:
                current_fen = board.fen()
                
                if board.turn == chess.BLACK:
                    cvs_move, _ = cvs.get_move(current_fen, movetime_ms=SEARCH_TIME_MS)
                    sf_move, sf_info = sf.get_move(current_fen, depth=SF_DEPTH)
                    
                    cvs_score = sf.evaluate_move(current_fen, cvs_move, depth=SF_DEPTH)
                    sf_score = sf.evaluate_move(current_fen, sf_move, depth=SF_DEPTH)
                    
                    diff = sf_score - cvs_score
                    if diff >= MISTAKE_THRESHOLD_CP:
                        print(f"[*] MISTAKE DETECTED! CVS chose {cvs_move} (eval {cvs_score}), Stockfish prefers {sf_move} (eval {sf_score}). Diff: {diff}cp")
                        
                        print("  Running Branch A (CVS mistake path)...")
                        play_rollout(current_fen, [cvs_move], BRANCH_DEPTH_PLIES, cvs, sf, record_position)
                        
                        print("  Running Branch B (Stockfish oracle path)...")
                        play_rollout(current_fen, [sf_move], BRANCH_DEPTH_PLIES, cvs, sf, record_position)

                    board.push(chess.Move.from_uci(cvs_move))
                else:
                    sf_move, _ = sf.get_move(current_fen, depth=SF_DEPTH)
                    board.push(chess.Move.from_uci(sf_move))

            print(f"Game {game_idx+1} finished. Total FENs collected so far: {len(collected_fens)}")
    except KeyboardInterrupt:
        print("\nInterrupted by user. Exiting cleanly...")
    finally:
        cvs.close()
        sf.close()
        out_file.close()
        print(f"Finished. Total FENs saved in {OUT_PATH}: {len(collected_fens)}")

if __name__ == '__main__':
    main()
