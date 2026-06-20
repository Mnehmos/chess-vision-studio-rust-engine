import unittest

from bench_equal_time import (
    bootstrap_mean_ci,
    checkpoint_identity,
    paired_summary,
    stable_final_depth,
)
from bench_forensic_time import transition_summary


class EqualTimeTests(unittest.TestCase):
    def test_forensic_transition_distinguishes_first_and_sustained_clear(self):
        rows = []
        moves = {
            10: ("bad", "bad"),
            25: ("safe", "bad"),
            50: ("safe", "safe"),
            100: ("bad", "bad"),
            250: ("safe", "safe"),
        }
        for budget, (raw, hybrid) in moves.items():
            rows.append(
                {
                    "budgetMs": budget,
                    "raw": {"move": raw},
                    "hybrid": {"move": hybrid},
                }
            )
        _, first, sustained = transition_summary(
            rows,
            list(moves),
            "bad",
        )
        self.assertEqual(first, {"raw": 25, "hybrid": 50})
        self.assertEqual(sustained, {"raw": 250, "hybrid": 250})

    def test_checkpoint_identity_uses_suite_name_and_policy_hashes(self):
        raw = {"id": "raw", "policy_sha": "raw-policy"}
        hybrid = {"id": "hybrid", "policy_sha": "hybrid-policy"}
        suite = {"name": "clean", "hash": "suite-hash"}
        identity = checkpoint_identity(
            raw,
            hybrid,
            suite,
            92,
            [25, 50],
            "seed",
        )
        self.assertEqual(identity["suite"], "clean")
        self.assertEqual(identity["rawPolicySha"], "raw-policy")
        self.assertEqual(identity["hybridPolicySha"], "hybrid-policy")

    def test_stable_final_depth_requires_the_move_to_remain_final(self):
        iterations = [
            {"depth": 1, "uci": "e2e4"},
            {"depth": 2, "uci": "d2d4"},
            {"depth": 3, "uci": "e2e4"},
            {"depth": 4, "uci": "e2e4"},
        ]
        self.assertEqual(stable_final_depth(iterations, "e2e4"), 3)

    def test_bootstrap_is_deterministic(self):
        first = bootstrap_mean_ci([-10, 0, 5, 20], "seed", samples=1000)
        second = bootstrap_mean_ci([-10, 0, 5, 20], "seed", samples=1000)
        self.assertEqual(first, second)

    def test_summary_preserves_catastrophic_saves(self):
        rows = [
            {
                "raw": {"cpLoss": 1200, "depth": 8, "nodes": 100},
                "hybrid": {"cpLoss": 0, "depth": 7, "nodes": 200},
            },
            {
                "raw": {"cpLoss": 10, "depth": 8, "nodes": 100},
                "hybrid": {"cpLoss": 30, "depth": 7, "nodes": 200},
            },
        ]
        summary = paired_summary(rows, "seed")
        self.assertEqual(summary["catastrophicSaves1000"], 1)
        self.assertEqual(summary["catastrophicRegressions1000"], 0)
        self.assertEqual(summary["hybridWins"], 1)
        self.assertEqual(summary["rawWins"], 1)


if __name__ == "__main__":
    unittest.main()
