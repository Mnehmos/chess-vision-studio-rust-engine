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
GEN8_EXE = 'f:/tools/cvs-baselines/uci-gen8v2-champion.exe'
CVS_EXE = 'target/release/uci.exe'

BASE_W = 'f:/Github/chess-vision-studio/arena/out/value-weights-mixed.json'
RUNG2_W = 'f:/Github/chess-vision-studio/arena/out/rung2-weights-mixed.json'

GEN8_NET = 'f:/Github/chess-vision-studio/arena/out/gen8-raw-h256-v2.json'
GEN9_NET = 'f:/Github/chess-vision-studio/arena/out/gen9-raw-h256-d20.json'

BOOK_PATH = 'F:/tools/openings.epd'
OUT_PATH = 'F:/tools/gen9-disagreement-seeds.jsonl'

# Game settings
SEARCH_TIME_MS = 100
SF_DEPTH = 15
SF_LABEL_DEPTH = 20
BRANCH_DEPTH_PLIES = 20
MISTAKE_THRESHOLD_CP = 50

# Value table for cheap SEE proxy
VAL = {
    chess.PAWN: 100,
    chess.KNIGHT: 320,
    chess.BISHOP: 330,
    chess.ROOK: 500,
    chess.QUEEN: 900,
    chess.KING: 20000
}

def has_winning_capture(board):
    for mv in board.legal_moves:
        if not board.is_capture(mv):
            continue
        victim = board.piece_type_at(mv.to_square)
        if victim is None:  # en passant
            victim = chess.PAWN
        attacker = board.piece_type_at(mv.from_square)
        # Cheap SEE proxy: winning if victim >= attacker, or target undefended.
        if VAL[victim] >= VAL[attacker]:
            return True
        if not board.is_attacked_by(not board.turn, mv.to_square):
            return True
    return False

def is_quiet(board):
    return not board.is_check() and not has_winning_capture(board)

class EngineProcess:
    def __init__(self, cmd, net_path=None, extra_args=None, is_sf=False):
        self.is_sf = is_sf
        args = [cmd]
        if not is_sf:
            args += ['--base', BASE_W, '--rung2', RUNG2_W]
            if net_path:
                args += ['--nnue', net_path]
            if extra_args:
                args += extra_args
        
        self.p = subprocess.Popen(args, stdin=subprocess.PIPE, stdout=subprocess.PIPE, text=True, bufsize=1)
        self._send('uci')
        if not is_sf:
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
                parts = line.split()
                if len(parts) > 1:
                    bestmove = parts[1]
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

def play_rollout(fen, moves_prefix, steps, engine_variant, sf, record_callback):
    board = chess.Board(fen)
    for mv in moves_prefix:
        try:
            board.push(chess.Move.from_uci(mv))
        except Exception:
            return

    for step in range(steps):
        if board.is_game_over():
            break
        
        current_fen = board.fen()
        if board.turn == chess.WHITE:
            mv_uci, _ = sf.get_move(current_fen, depth=SF_DEPTH)
        else:
            mv_uci, _ = engine_variant.get_move(current_fen, movetime_ms=SEARCH_TIME_MS)

        if not mv_uci or mv_uci == '0000':
            break

        # Check quiet status before push on fresh board instance of current_fen
        eval_board = chess.Board(current_fen)
        if is_quiet(eval_board):
            record_callback(current_fen)

        try:
            board.push(chess.Move.from_uci(mv_uci))
        except Exception:
            break

def main():
    print("Initializing engine processes...")
    sf = EngineProcess(SF_EXE, is_sf=True)
    
    # Define and launch all 9 engine configurations
    gen8_flags = ['--futility', '--rfp', '--tt-prune-store', '--qtt', '--histmalus', '--histlmr']
    engines = {
        'gen8': EngineProcess(GEN8_EXE, net_path=GEN8_NET, extra_args=gen8_flags),
        'gen9': EngineProcess(CVS_EXE, net_path=GEN9_NET),
        'gen8_cvs_nnue': EngineProcess(CVS_EXE, net_path=GEN8_NET),
        'spec_king': EngineProcess(CVS_EXE, net_path=GEN9_NET, extra_args=['--lane', 'king']),
        'spec_see': EngineProcess(CVS_EXE, net_path=GEN9_NET, extra_args=['--lane', 'see']),
        'spec_tactics': EngineProcess(CVS_EXE, net_path=GEN9_NET, extra_args=['--lane', 'tactics']),
        'spec_defender': EngineProcess(CVS_EXE, net_path=GEN9_NET, extra_args=['--lane', 'defender']),
        'spec_quietdef': EngineProcess(CVS_EXE, net_path=GEN9_NET, extra_args=['--lane', 'quietdef']),
        'spec_pawn': EngineProcess(CVS_EXE, net_path=GEN9_NET, extra_args=['--lane', 'pawn']),
    }
    
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
            # Alternating player engine in main games
            player_name = 'gen8' if game_idx % 2 == 0 else 'gen9'
            player = engines[player_name]
            
            start_fen = random.choice(book)
            board = chess.Board(start_fen)
            print(f"\n--- Game {game_idx+1}/{games_to_play} ({player_name} vs Stockfish) starting from {start_fen} ---")

            while not board.is_game_over() and len(board.move_stack) < 80:
                current_fen = board.fen()
                
                if board.turn == chess.BLACK:
                    engine_move, _ = player.get_move(current_fen, movetime_ms=SEARCH_TIME_MS)
                    sf_move, sf_info = sf.get_move(current_fen, depth=SF_DEPTH)
                    
                    if not engine_move or engine_move == '0000':
                        break

                    engine_score = sf.evaluate_move(current_fen, engine_move, depth=SF_DEPTH)
                    sf_score = sf.evaluate_move(current_fen, sf_move, depth=SF_DEPTH)
                    
                    diff = sf_score - engine_score
                    if diff >= MISTAKE_THRESHOLD_CP:
                        print(f"[*] MISTAKE DETECTED! {player_name} chose {engine_move} (eval {engine_score}), Stockfish prefers {sf_move} (eval {sf_score}). Diff: {diff}cp")
                        
                        # Run branching rollouts under ALL engine variants!
                        for name, eng in engines.items():
                            print(f"  [{name}] Running Branch A (mistake path)...")
                            play_rollout(current_fen, [engine_move], BRANCH_DEPTH_PLIES, eng, sf, record_position)
                            
                            print(f"  [{name}] Running Branch B (Stockfish oracle path)...")
                            play_rollout(current_fen, [sf_move], BRANCH_DEPTH_PLIES, eng, sf, record_position)

                    board.push(chess.Move.from_uci(engine_move))
                else:
                    sf_move, _ = sf.get_move(current_fen, depth=SF_DEPTH)
                    if not sf_move or sf_move == '0000':
                        break
                    board.push(chess.Move.from_uci(sf_move))

            print(f"Game {game_idx+1} finished. Total FENs collected so far: {len(collected_fens)}")
    except KeyboardInterrupt:
        print("\nInterrupted by user. Exiting cleanly...")
    finally:
        sf.close()
        for name, eng in engines.items():
            eng.close()
        out_file.close()
        print(f"Finished. Total FENs saved in {OUT_PATH}: {len(collected_fens)}")

if __name__ == '__main__':
    main()
