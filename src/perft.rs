//! Perft — count leaf nodes of the legal move tree to a fixed depth. The standard
//! correctness oracle for a move generator + make/unmake.
use crate::movegen::generate_legal;
use crate::{Move, Position};

/// Count legal leaf nodes at `depth`.
pub fn perft(pos: &mut Position, depth: u32) -> u64 {
    if depth == 0 {
        return 1;
    }
    let moves = generate_legal(pos);
    if depth == 1 {
        return moves.len() as u64;
    }
    let mut nodes = 0;
    for mv in moves {
        pos.make(mv);
        nodes += perft(pos, depth - 1);
        pos.unmake();
    }
    nodes
}

/// Per-root-move perft (for debugging mismatches against a reference).
pub fn perft_divide(pos: &mut Position, depth: u32) -> Vec<(Move, u64)> {
    let moves = generate_legal(pos);
    let mut out = Vec::with_capacity(moves.len());
    for mv in moves {
        pos.make(mv);
        let n = if depth <= 1 { 1 } else { perft(pos, depth - 1) };
        pos.unmake();
        out.push((mv, n));
    }
    out
}
