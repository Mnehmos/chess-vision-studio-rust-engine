import tempfile
import unittest
from pathlib import Path

from rsi_run_loop import fen_key, load_reserved


class RsiReservationTests(unittest.TestCase):
    def test_fen_key_preserves_legal_state_and_ignores_clocks(self):
        self.assertEqual(
            fen_key("8/8/8/8/8/8/8/K6k w - - 17 42"),
            "8/8/8/8/8/8/8/K6k w - -",
        )

    def test_load_reserved_deduplicates_clock_variants(self):
        with tempfile.TemporaryDirectory() as temp:
            path = Path(temp) / "reserved.txt"
            path.write_text(
                "8/8/8/8/8/8/8/K6k w - - 0 1\n"
                "8/8/8/8/8/8/8/K6k w - - 17 42\n",
                encoding="utf8",
            )
            reserved = load_reserved(path)
        self.assertEqual(reserved, {"8/8/8/8/8/8/8/K6k w - -"})


if __name__ == "__main__":
    unittest.main()
