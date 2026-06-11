//! Lock-free shared transposition table for lazy SMP.
//!
//! Each slot is two `AtomicU64`s: the entry packed into one word, and the
//! position hash XORed with that word (Hyatt's lockless-hashing trick). A torn
//! read/write — two threads racing on the same slot — fails the XOR check and
//! probes as a miss instead of returning a corrupt entry. All loads/stores are
//! `Relaxed`: the TT is a heuristic cache; stale or lost entries only cost
//! re-search, never correctness.
//!
//! Packed layout (low → high): score 32 bits (i32) · move 16 bits (0 = none) ·
//! depth 6 bits (clamped 0..63) · flag 2 bits · generation 6 bits (mod 64) ·
//! lane 2 bits (specialist-lane provenance: Fast/King/See/Tactics).
use crate::{Move, MoveFlag};
use std::sync::atomic::{AtomicU64, AtomicU8, Ordering};

#[derive(Clone, Copy, PartialEq, Debug)]
pub enum Flag {
    Exact,
    Lower,
    Upper,
}

#[derive(Clone, Copy)]
pub struct TtEntry {
    pub depth: i32,
    pub score: i32,
    pub flag: Flag,
    pub mv: Option<Move>,
    pub generation: u8,
    /// Specialist-lane provenance (0=Fast 1=King 2=See 3=Tactics) — lets the
    /// main thread measure which lane's discoveries it consumed.
    pub lane: u8,
}

const FLAGS: [MoveFlag; 14] = [
    MoveFlag::Quiet,
    MoveFlag::DoublePush,
    MoveFlag::KingCastle,
    MoveFlag::QueenCastle,
    MoveFlag::Capture,
    MoveFlag::EnPassant,
    MoveFlag::PromoN,
    MoveFlag::PromoB,
    MoveFlag::PromoR,
    MoveFlag::PromoQ,
    MoveFlag::PromoNCap,
    MoveFlag::PromoBCap,
    MoveFlag::PromoRCap,
    MoveFlag::PromoQCap,
];

#[inline]
fn pack_move(mv: Option<Move>) -> u64 {
    match mv {
        // +1 so that a real move never packs to 0 (the None sentinel).
        Some(m) => {
            let flag_idx = FLAGS.iter().position(|f| *f == m.flag).unwrap() as u64;
            1 + ((m.from as u64) | ((m.to as u64) << 6) | (flag_idx << 12))
        }
        None => 0,
    }
}

#[inline]
fn unpack_move(bits: u64) -> Option<Move> {
    if bits == 0 {
        return None;
    }
    let b = bits - 1;
    Some(Move {
        from: (b & 63) as u8,
        to: ((b >> 6) & 63) as u8,
        flag: FLAGS[((b >> 12) & 15) as usize],
    })
}

#[inline]
fn pack(e: &TtEntry) -> u64 {
    (e.score as u32 as u64)
        | (pack_move(e.mv) << 32)
        | ((e.depth.clamp(0, 63) as u64) << 48)
        | ((e.flag as u64) << 54)
        | (((e.generation & 63) as u64) << 56)
        | (((e.lane & 3) as u64) << 62)
}

#[inline]
fn unpack(d: u64) -> TtEntry {
    TtEntry {
        score: d as u32 as i32,
        mv: unpack_move((d >> 32) & 0xFFFF),
        depth: ((d >> 48) & 63) as i32,
        flag: match (d >> 54) & 3 {
            0 => Flag::Exact,
            1 => Flag::Lower,
            _ => Flag::Upper,
        },
        generation: ((d >> 56) & 63) as u8,
        lane: ((d >> 62) & 3) as u8,
    }
}

pub struct SharedTt {
    /// (hash ^ data, data) per slot.
    slots: Vec<[AtomicU64; 2]>,
    mask: u64,
    generation: AtomicU8,
}

impl SharedTt {
    pub fn new(bits: u32) -> SharedTt {
        let size = 1usize << bits;
        let mut slots = Vec::with_capacity(size);
        slots.resize_with(size, || [AtomicU64::new(0), AtomicU64::new(0)]);
        SharedTt {
            slots,
            mask: (size as u64) - 1,
            generation: AtomicU8::new(0),
        }
    }

    /// Bump the search generation (main thread, once per `search()`).
    pub fn new_generation(&self) -> u8 {
        self.generation
            .fetch_add(1, Ordering::Relaxed)
            .wrapping_add(1)
            & 63
    }

    pub fn probe(&self, hash: u64) -> Option<TtEntry> {
        let s = &self.slots[(hash & self.mask) as usize];
        let key = s[0].load(Ordering::Relaxed);
        let data = s[1].load(Ordering::Relaxed);
        if data != 0 && key ^ data == hash {
            Some(unpack(data))
        } else {
            None
        }
    }

    /// Same replacement policy as the single-threaded table: same position
    /// keeps the deeper entry; older generations always lose; otherwise depth
    /// decides.
    #[allow(clippy::too_many_arguments)]
    pub fn store(
        &self,
        hash: u64,
        depth: i32,
        score: i32,
        flag: Flag,
        mv: Option<Move>,
        generation: u8,
        lane: u8,
    ) {
        let s = &self.slots[(hash & self.mask) as usize];
        let cur_key = s[0].load(Ordering::Relaxed);
        let cur_data = s[1].load(Ordering::Relaxed);
        let replace = if cur_data == 0 {
            true
        } else {
            let cur = unpack(cur_data);
            let same = cur_key ^ cur_data == hash;
            same && depth >= cur.depth
                || !same && (cur.generation != generation || depth >= cur.depth)
        };
        if !replace {
            return;
        }
        let data = pack(&TtEntry {
            depth,
            score,
            flag,
            mv,
            generation,
            lane,
        });
        s[1].store(data, Ordering::Relaxed);
        s[0].store(hash ^ data, Ordering::Relaxed);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn move_roundtrip() {
        for flag in FLAGS {
            let m = Move {
                from: 12,
                to: 28,
                flag,
            };
            assert_eq!(unpack_move(pack_move(Some(m))), Some(m));
        }
        assert_eq!(unpack_move(pack_move(None)), None);
        // a1a1 quiet must not collide with the None sentinel.
        let zero = Move {
            from: 0,
            to: 0,
            flag: MoveFlag::Quiet,
        };
        assert_eq!(unpack_move(pack_move(Some(zero))), Some(zero));
    }

    #[test]
    fn entry_roundtrip_extremes() {
        for score in [0, 1, -1, 999_983, -999_983, i32::MIN / 4] {
            let e = TtEntry {
                depth: 63,
                score,
                flag: Flag::Lower,
                mv: Some(Move {
                    from: 63,
                    to: 0,
                    flag: MoveFlag::PromoQCap,
                }),
                generation: 63,
                lane: 3,
            };
            let u = unpack(pack(&e));
            assert_eq!(u.score, score);
            assert_eq!(u.depth, 63);
            assert_eq!(u.mv, e.mv);
            assert_eq!(u.generation, 63);
            assert_eq!(u.lane, 3);
        }
    }

    #[test]
    fn probe_store_xor_check() {
        let tt = SharedTt::new(4);
        let h = 0xDEAD_BEEF_1234_5678u64;
        tt.store(h, 8, 42, Flag::Exact, None, 1, 2);
        assert_eq!(tt.probe(h).unwrap().lane, 2);
        let e = tt.probe(h).unwrap();
        assert_eq!((e.depth, e.score), (8, 42));
        // A different hash mapping to the same slot must miss.
        assert!(tt.probe(h ^ 0x10).is_none());
    }
}
