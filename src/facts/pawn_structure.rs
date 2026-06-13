use crate::facts::piece_safety::piece_ref;
use crate::facts::position::{side, square_name};
use crate::facts::types::{
    DoubledPawnFact, FactCollection, PawnIslandFact, PawnStructureFacts, StructureDelta,
};
use crate::{file_of, rank_of, Color, Piece, Position};
use std::collections::BTreeMap;

pub fn pawn_structure_facts(pos: &Position) -> PawnStructureFacts {
    let mut doubled = Vec::new();
    let mut isolated = Vec::new();
    let mut passed = Vec::new();
    let mut islands = Vec::new();

    for color in [Color::White, Color::Black] {
        let pawn_squares = squares(pos.pieces[color.index()][Piece::Pawn.index()]);
        let enemy_squares = squares(pos.pieces[color.flip().index()][Piece::Pawn.index()]);
        let mut by_file = BTreeMap::<u8, Vec<u8>>::new();
        for &sq in &pawn_squares {
            by_file.entry(file_of(sq)).or_default().push(sq);
        }
        for file_squares in by_file.values_mut() {
            file_squares.sort_unstable();
        }

        for (&file, file_squares) in &by_file {
            if file_squares.len() > 1 {
                doubled.push(DoubledPawnFact {
                    id: format!(
                        "{}-doubled-pawns-{}-{}",
                        side_name(color),
                        file_name(file),
                        file_squares
                            .iter()
                            .map(|sq| square_name(*sq))
                            .collect::<Vec<_>>()
                            .join("-")
                    ),
                    side: side(color),
                    file: file_name(file),
                    squares: file_squares.iter().map(|sq| square_name(*sq)).collect(),
                });
            }
        }

        for &sq in &pawn_squares {
            let file = file_of(sq);
            let has_adjacent = [file.checked_sub(1), (file < 7).then_some(file + 1)]
                .into_iter()
                .flatten()
                .any(|adjacent| by_file.contains_key(&adjacent));
            if !has_adjacent {
                isolated.push(piece_ref(color, Piece::Pawn, sq));
            }
            if is_passed(color, sq, &enemy_squares) {
                passed.push(piece_ref(color, Piece::Pawn, sq));
            }
        }

        let occupied_files: Vec<u8> = by_file.keys().copied().collect();
        let mut start = 0;
        while start < occupied_files.len() {
            let mut end = start;
            while end + 1 < occupied_files.len()
                && occupied_files[end + 1] == occupied_files[end] + 1
            {
                end += 1;
            }
            let files = &occupied_files[start..=end];
            let island_squares: Vec<u8> = files
                .iter()
                .flat_map(|file| by_file.get(file).into_iter().flatten().copied())
                .collect();
            islands.push(PawnIslandFact {
                id: format!(
                    "{}-pawn-island-{}",
                    side_name(color),
                    files
                        .iter()
                        .map(|file| file_name(*file))
                        .collect::<Vec<_>>()
                        .join("")
                ),
                side: side(color),
                files: files.iter().map(|file| file_name(*file)).collect(),
                squares: island_squares.iter().map(|sq| square_name(*sq)).collect(),
            });
            start = end + 1;
        }
    }

    isolated.sort_by(|a, b| a.id.cmp(&b.id));
    passed.sort_by(|a, b| a.id.cmp(&b.id));
    doubled.sort_by(|a, b| a.id.cmp(&b.id));
    islands.sort_by(|a, b| a.id.cmp(&b.id));
    PawnStructureFacts {
        doubled,
        isolated,
        passed,
        islands,
        backward: FactCollection::uncomputed("not_in_milestone_1"),
        connected_passed: FactCollection::uncomputed("not_in_milestone_1"),
        open_files: FactCollection::uncomputed("not_in_milestone_1"),
        semi_open_files: FactCollection::uncomputed("not_in_milestone_1"),
        king_shield_missing: FactCollection::uncomputed("not_in_milestone_1"),
        pawn_chains: FactCollection::uncomputed("not_in_milestone_1"),
    }
}

pub fn structure_deltas(
    before: &PawnStructureFacts,
    after: &PawnStructureFacts,
) -> (Vec<StructureDelta>, Vec<StructureDelta>) {
    let before_map = structures(before);
    let after_map = structures(after);
    let created = after_map
        .iter()
        .filter(|(id, _)| !before_map.contains_key(*id))
        .map(|(_, delta)| delta.clone())
        .collect();
    let removed = before_map
        .iter()
        .filter(|(id, _)| !after_map.contains_key(*id))
        .map(|(_, delta)| delta.clone())
        .collect();
    (created, removed)
}

fn structures(facts: &PawnStructureFacts) -> BTreeMap<String, StructureDelta> {
    let mut out = BTreeMap::new();
    for fact in &facts.doubled {
        out.insert(
            fact.id.clone(),
            StructureDelta {
                fact_id: fact.id.clone(),
                kind: "doubled_pawns".into(),
                side: fact.side,
                squares: fact.squares.clone(),
            },
        );
    }
    for fact in &facts.isolated {
        out.insert(
            format!("isolated-{}", fact.id),
            StructureDelta {
                fact_id: format!("isolated-{}", fact.id),
                kind: "isolated_pawn".into(),
                side: fact.side,
                squares: vec![fact.square.clone()],
            },
        );
    }
    for fact in &facts.passed {
        out.insert(
            format!("passed-{}", fact.id),
            StructureDelta {
                fact_id: format!("passed-{}", fact.id),
                kind: "passed_pawn".into(),
                side: fact.side,
                squares: vec![fact.square.clone()],
            },
        );
    }
    out
}

fn is_passed(color: Color, square: u8, enemy_pawns: &[u8]) -> bool {
    let file = file_of(square) as i8;
    let rank = rank_of(square);
    !enemy_pawns.iter().any(|enemy| {
        let enemy_file = file_of(*enemy) as i8;
        let enemy_rank = rank_of(*enemy);
        (enemy_file - file).abs() <= 1
            && match color {
                Color::White => enemy_rank > rank,
                Color::Black => enemy_rank < rank,
            }
    })
}

fn squares(mut bb: u64) -> Vec<u8> {
    let mut out = Vec::new();
    while bb != 0 {
        let sq = bb.trailing_zeros() as u8;
        bb &= bb - 1;
        out.push(sq);
    }
    out
}

fn file_name(file: u8) -> String {
    ((b'a' + file) as char).to_string()
}

fn side_name(color: Color) -> &'static str {
    match color {
        Color::White => "white",
        Color::Black => "black",
    }
}
