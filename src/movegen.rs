//! Legal move generation. Pseudo-legal moves are generated with bitboards, then
//! filtered for legality by make → "is the mover's king attacked?" → unmake. This
//! is the simplest provably-correct scheme; pin-aware legal gen can come later.
use crate::attacks::{
    bishop_attacks, is_square_attacked, king_attacks, knight_attacks, pawn_attacks, queen_attacks,
    rook_attacks,
};
use crate::{castle, rank_of, Color, Move, MoveFlag, Piece, Position};

/// Conservative fixed move-list capacity. The legal-move maximum is below this
/// in chess, and keeping search move lists on the stack avoids per-node heap
/// allocation in the hot path.
pub const MAX_MOVES: usize = 256;
const EMPTY_MOVE: Move = Move {
    from: 0,
    to: 0,
    flag: MoveFlag::Quiet,
};

#[derive(Clone)]
pub struct MoveList {
    moves: [Move; MAX_MOVES],
    len: usize,
}

impl Default for MoveList {
    #[inline]
    fn default() -> Self {
        Self::new()
    }
}

impl MoveList {
    #[inline]
    pub fn new() -> MoveList {
        MoveList {
            moves: [EMPTY_MOVE; MAX_MOVES],
            len: 0,
        }
    }

    #[inline]
    pub fn push(&mut self, mv: Move) {
        assert!(self.len < MAX_MOVES, "move list overflow");
        self.moves[self.len] = mv;
        self.len += 1;
    }

    #[inline]
    pub fn len(&self) -> usize {
        self.len
    }

    #[inline]
    pub fn is_empty(&self) -> bool {
        self.len == 0
    }

    #[inline]
    pub fn get(&self, index: usize) -> Move {
        debug_assert!(index < self.len);
        self.moves[index]
    }

    #[inline]
    pub fn as_slice(&self) -> &[Move] {
        &self.moves[..self.len]
    }

    #[inline]
    pub fn as_mut_slice(&mut self) -> &mut [Move] {
        &mut self.moves[..self.len]
    }

    pub fn retain<F>(&mut self, mut keep: F)
    where
        F: FnMut(&Move) -> bool,
    {
        let mut out = 0usize;
        for i in 0..self.len {
            let mv = self.moves[i];
            if keep(&mv) {
                self.moves[out] = mv;
                out += 1;
            }
        }
        self.len = out;
    }

    pub fn into_vec(self) -> Vec<Move> {
        self.as_slice().to_vec()
    }
}

trait MoveSink {
    fn push_move(&mut self, mv: Move);
}

impl MoveSink for Vec<Move> {
    #[inline]
    fn push_move(&mut self, mv: Move) {
        self.push(mv);
    }
}

impl MoveSink for MoveList {
    #[inline]
    fn push_move(&mut self, mv: Move) {
        self.push(mv);
    }
}

#[inline]
fn pop_lsb(b: &mut u64) -> u8 {
    let s = b.trailing_zeros() as u8;
    *b &= *b - 1;
    s
}

#[inline]
fn add_quiet_or_cap<S: MoveSink>(moves: &mut S, from: u8, mut targets: u64, them_occ: u64) {
    while targets != 0 {
        let to = pop_lsb(&mut targets);
        let flag = if (1u64 << to) & them_occ != 0 {
            MoveFlag::Capture
        } else {
            MoveFlag::Quiet
        };
        moves.push_move(Move::new(from, to, flag));
    }
}

const PROMOS: [MoveFlag; 4] = [
    MoveFlag::PromoQ,
    MoveFlag::PromoR,
    MoveFlag::PromoB,
    MoveFlag::PromoN,
];
const PROMO_CAPS: [MoveFlag; 4] = [
    MoveFlag::PromoQCap,
    MoveFlag::PromoRCap,
    MoveFlag::PromoBCap,
    MoveFlag::PromoNCap,
];

fn gen_pawns<S: MoveSink>(pos: &Position, moves: &mut S) {
    let us = pos.stm;
    let them = us.flip();
    let them_occ = pos.occ[them.index()];
    let empty = !pos.all;
    let white = us == Color::White;
    let promo_rank = if white { 7 } else { 0 };
    let start_rank = if white { 1 } else { 6 };

    let mut pawns = pos.pieces[us.index()][Piece::Pawn.index()];
    while pawns != 0 {
        let from = pop_lsb(&mut pawns);
        let one = if white { from + 8 } else { from - 8 };
        // Single push (to empty).
        if empty & (1u64 << one) != 0 {
            if rank_of(one) == promo_rank {
                for f in PROMOS {
                    moves.push_move(Move::new(from, one, f));
                }
            } else {
                moves.push_move(Move::new(from, one, MoveFlag::Quiet));
                // Double push.
                if rank_of(from) == start_rank {
                    let two = if white { from + 16 } else { from - 16 };
                    if empty & (1u64 << two) != 0 {
                        moves.push_move(Move::new(from, two, MoveFlag::DoublePush));
                    }
                }
            }
        }
        // Captures.
        let mut caps = pawn_attacks(us, from) & them_occ;
        while caps != 0 {
            let to = pop_lsb(&mut caps);
            if rank_of(to) == promo_rank {
                for f in PROMO_CAPS {
                    moves.push_move(Move::new(from, to, f));
                }
            } else {
                moves.push_move(Move::new(from, to, MoveFlag::Capture));
            }
        }
        // En passant.
        if let Some(ep) = pos.ep {
            if pawn_attacks(us, from) & (1u64 << ep) != 0 {
                moves.push_move(Move::new(from, ep, MoveFlag::EnPassant));
            }
        }
    }
}

fn gen_leapers_and_sliders<S: MoveSink>(pos: &Position, moves: &mut S) {
    let us = pos.stm;
    let them_occ = pos.occ[them_index(us)];
    let not_us = !pos.occ[us.index()];
    let occ = pos.all;

    let mut kn = pos.pieces[us.index()][Piece::Knight.index()];
    while kn != 0 {
        let from = pop_lsb(&mut kn);
        add_quiet_or_cap(moves, from, knight_attacks(from) & not_us, them_occ);
    }
    let mut bi = pos.pieces[us.index()][Piece::Bishop.index()];
    while bi != 0 {
        let from = pop_lsb(&mut bi);
        add_quiet_or_cap(moves, from, bishop_attacks(from, occ) & not_us, them_occ);
    }
    let mut ro = pos.pieces[us.index()][Piece::Rook.index()];
    while ro != 0 {
        let from = pop_lsb(&mut ro);
        add_quiet_or_cap(moves, from, rook_attacks(from, occ) & not_us, them_occ);
    }
    let mut qu = pos.pieces[us.index()][Piece::Queen.index()];
    while qu != 0 {
        let from = pop_lsb(&mut qu);
        add_quiet_or_cap(moves, from, queen_attacks(from, occ) & not_us, them_occ);
    }
    let mut ki = pos.pieces[us.index()][Piece::King.index()];
    while ki != 0 {
        let from = pop_lsb(&mut ki);
        add_quiet_or_cap(moves, from, king_attacks(from) & not_us, them_occ);
    }
}

#[inline]
fn them_index(us: Color) -> usize {
    us.flip().index()
}

fn gen_castling<S: MoveSink>(pos: &Position, moves: &mut S) {
    let us = pos.stm;
    let them = us.flip();
    let occ = pos.all;
    // (kingside_right, queenside_right, e, f, g, b, c, d, g_to, c_to)
    let (kr, qr, e, f, g, b, c, d) = if us == Color::White {
        (castle::WK, castle::WQ, 4u8, 5u8, 6u8, 1u8, 2u8, 3u8)
    } else {
        (castle::BK, castle::BQ, 60u8, 61u8, 62u8, 57u8, 58u8, 59u8)
    };
    let attacked = |sq: u8| is_square_attacked(pos, sq, them, occ);
    let empty = |sq: u8| occ & (1u64 << sq) == 0;

    // Kingside: f,g empty; king path e,f,g safe.
    if pos.castling & kr != 0
        && empty(f)
        && empty(g)
        && !attacked(e)
        && !attacked(f)
        && !attacked(g)
    {
        moves.push_move(Move::new(e, g, MoveFlag::KingCastle));
    }
    // Queenside: b,c,d empty; king path e,d,c safe.
    if pos.castling & qr != 0
        && empty(b)
        && empty(c)
        && empty(d)
        && !attacked(e)
        && !attacked(d)
        && !attacked(c)
    {
        moves.push_move(Move::new(e, c, MoveFlag::QueenCastle));
    }
}

/// All pseudo-legal moves (may leave the king in check; not castling-through-check).
pub fn generate_pseudo(pos: &Position, moves: &mut Vec<Move>) {
    gen_pawns(pos, moves);
    gen_leapers_and_sliders(pos, moves);
    gen_castling(pos, moves);
}

fn generate_pseudo_list(pos: &Position) -> MoveList {
    let mut moves = MoveList::new();
    gen_pawns(pos, &mut moves);
    gen_leapers_and_sliders(pos, &mut moves);
    gen_castling(pos, &mut moves);
    moves
}

/// Pseudo-legal NOISY moves only: captures, promotions (incl. push promotions)
/// and en passant — the quiescence workload (Search Patch 6). Castling and
/// quiet moves are never noisy.
pub fn generate_pseudo_noisy(pos: &Position, moves: &mut Vec<Move>) {
    generate_pseudo_noisy_into(pos, moves);
}

fn generate_pseudo_noisy_into<S: MoveSink>(pos: &Position, moves: &mut S) {
    let us = pos.stm;
    let them = us.flip();
    let them_occ = pos.occ[them.index()];
    let empty = !pos.all;
    let white = us == Color::White;
    let promo_rank = if white { 7 } else { 0 };

    let mut pawns = pos.pieces[us.index()][Piece::Pawn.index()];
    while pawns != 0 {
        let from = pop_lsb(&mut pawns);
        let one = if white { from + 8 } else { from - 8 };
        // Push promotions are forcing — they belong in quiescence.
        if rank_of(one) == promo_rank && empty & (1u64 << one) != 0 {
            for f in PROMOS {
                moves.push_move(Move::new(from, one, f));
            }
        }
        let mut caps = pawn_attacks(us, from) & them_occ;
        while caps != 0 {
            let to = pop_lsb(&mut caps);
            if rank_of(to) == promo_rank {
                for f in PROMO_CAPS {
                    moves.push_move(Move::new(from, to, f));
                }
            } else {
                moves.push_move(Move::new(from, to, MoveFlag::Capture));
            }
        }
        if let Some(ep) = pos.ep {
            if pawn_attacks(us, from) & (1u64 << ep) != 0 {
                moves.push_move(Move::new(from, ep, MoveFlag::EnPassant));
            }
        }
    }

    let occ = pos.all;
    let mut kn = pos.pieces[us.index()][Piece::Knight.index()];
    while kn != 0 {
        let from = pop_lsb(&mut kn);
        add_quiet_or_cap(moves, from, knight_attacks(from) & them_occ, them_occ);
    }
    let mut bi = pos.pieces[us.index()][Piece::Bishop.index()];
    while bi != 0 {
        let from = pop_lsb(&mut bi);
        add_quiet_or_cap(moves, from, bishop_attacks(from, occ) & them_occ, them_occ);
    }
    let mut ro = pos.pieces[us.index()][Piece::Rook.index()];
    while ro != 0 {
        let from = pop_lsb(&mut ro);
        add_quiet_or_cap(moves, from, rook_attacks(from, occ) & them_occ, them_occ);
    }
    let mut qu = pos.pieces[us.index()][Piece::Queen.index()];
    while qu != 0 {
        let from = pop_lsb(&mut qu);
        add_quiet_or_cap(moves, from, queen_attacks(from, occ) & them_occ, them_occ);
    }
    let mut ki = pos.pieces[us.index()][Piece::King.index()];
    while ki != 0 {
        let from = pop_lsb(&mut ki);
        add_quiet_or_cap(moves, from, king_attacks(from) & them_occ, them_occ);
    }
}

fn generate_pseudo_noisy_list(pos: &Position) -> MoveList {
    let mut moves = MoveList::new();
    generate_pseudo_noisy_into(pos, &mut moves);
    moves
}

/// Strictly legal noisy moves for the side to move.
pub fn generate_legal_noisy(pos: &mut Position) -> Vec<Move> {
    generate_legal_noisy_list(pos).into_vec()
}

/// Strictly legal noisy moves for the side to move, in a stack-backed list.
pub fn generate_legal_noisy_list(pos: &mut Position) -> MoveList {
    let pseudo = generate_pseudo_noisy_list(pos);
    let us = pos.stm;
    let them = us.flip();
    let mut legal = MoveList::new();
    for i in 0..pseudo.len() {
        let mv = pseudo.get(i);
        pos.make(mv);
        let ksq = pos.king_sq(us);
        if !is_square_attacked(pos, ksq, them, pos.all) {
            legal.push(mv);
        }
        pos.unmake();
    }
    legal
}

/// Does the side to move have ANY legal move? Early-exits on the first one —
/// the cheap stalemate probe for noisy-only quiescence nodes.
pub fn has_legal_move(pos: &mut Position) -> bool {
    let pseudo = generate_pseudo_list(pos);
    let us = pos.stm;
    let them = us.flip();
    for i in 0..pseudo.len() {
        let mv = pseudo.get(i);
        pos.make(mv);
        let ksq = pos.king_sq(us);
        let ok = !is_square_attacked(pos, ksq, them, pos.all);
        pos.unmake();
        if ok {
            return true;
        }
    }
    false
}

/// All strictly legal moves for the side to move.
pub fn generate_legal(pos: &mut Position) -> Vec<Move> {
    generate_legal_list(pos).into_vec()
}

/// All strictly legal moves for the side to move, in a stack-backed list.
pub fn generate_legal_list(pos: &mut Position) -> MoveList {
    let pseudo = generate_pseudo_list(pos);
    let us = pos.stm;
    let them = us.flip();
    let mut legal = MoveList::new();
    for i in 0..pseudo.len() {
        let mv = pseudo.get(i);
        pos.make(mv);
        let ksq = pos.king_sq(us);
        if !is_square_attacked(pos, ksq, them, pos.all) {
            legal.push(mv);
        }
        pos.unmake();
    }
    legal
}

/// Is the side to move currently in check?
pub fn in_check(pos: &Position) -> bool {
    let us = pos.stm;
    is_square_attacked(pos, pos.king_sq(us), us.flip(), pos.all)
}

/// Does playing `mv` give check to the opponent? (make → test → unmake.)
pub fn gives_check(pos: &mut Position, mv: Move) -> bool {
    pos.make(mv);
    let opp = pos.stm; // after make, the opponent is to move
    let checked = is_square_attacked(pos, pos.king_sq(opp), opp.flip(), pos.all);
    pos.unmake();
    checked
}
