import json
import tempfile
import unittest
from pathlib import Path

from build_clean_holdout import (
    exclusion_files,
    position_key,
    scan_exclusions,
    select_rows,
)


class CleanHoldoutTests(unittest.TestCase):
    def test_position_key_ignores_move_clocks_only(self):
        self.assertEqual(
            position_key("8/8/8/8/8/8/8/K6k w - - 17 42"),
            "8/8/8/8/8/8/8/K6k w - -",
        )

    def test_scan_exclusions_matches_four_and_six_field_fens(self):
        candidates = [
            {
                "key": "8/8/8/8/8/8/8/K6k w - -",
                "fen": "8/8/8/8/8/8/8/K6k w - - 17 42",
            }
        ]
        with tempfile.TemporaryDirectory() as temp:
            path = Path(temp) / "train.jsonl"
            path.write_text(
                json.dumps({"fen": "8/8/8/8/8/8/8/K6k w - -"}) + "\n",
                encoding="utf8",
            )
            contaminated, reports = scan_exclusions(
                candidates,
                [{"path": path, "format": "jsonl", "generation": 9}],
            )
        self.assertEqual(contaminated, {candidates[0]["key"]})
        self.assertEqual(reports[0]["matchedCandidates"], 1)

    def test_selection_caps_games_and_separates_plies(self):
        rows = []
        for game in ("a", "b", "c"):
            for ply in range(12, 80, 10):
                rows.append(
                    {
                        "key": f"{game}-{ply}",
                        "gameId": game,
                        "ply": ply,
                        "phase": "middlegame",
                    }
                )
        selected = select_rows(
            rows,
            target=6,
            max_per_game=2,
            min_ply_gap=10,
            salt="test",
        )
        counts = {}
        for row in selected:
            counts[row["gameId"]] = counts.get(row["gameId"], 0) + 1
        self.assertEqual(len(selected), 6)
        self.assertTrue(all(value <= 2 for value in counts.values()))

    def test_exclusion_manifest_does_not_include_generated_holdout(self):
        files = exclusion_files(
            Path("benchmarks/holdout-exclusions.json").resolve()
        )
        names = {spec["path"].name for spec in files}
        self.assertNotIn("suite-clean-postmodel-20260619.txt", names)


if __name__ == "__main__":
    unittest.main()
