import unittest

from bench_equal_time import (
    bootstrap_mean_ci,
    checkpoint_identity,
    paired_summary,
    stable_final_depth,
)
from bench_forensic_time import transition_summary
from bench_sentinel_suite import summarize as summarize_sentinel_suite
from bench_tactical_sentinel import (
    all_repeats_transition,
    child_fen,
    evidence_class,
    sentinel_config,
)


class EqualTimeTests(unittest.TestCase):
    def test_sentinel_requires_independent_mate_confirmation(self):
        self.assertEqual(
            evidence_class({"mate": 4}, {"mate": -4}),
            "exact-mate",
        )
        self.assertEqual(
            evidence_class(
                {"mate": None, "scoreCp": 450},
                {"mate": None, "scoreCp": -420},
                {"move": "h2h4", "scoreCp": -300},
                "g2f3",
            ),
            "verified-major-loss",
        )
        self.assertEqual(
            evidence_class(
                {"mate": None, "scoreCp": 450},
                {"mate": None, "scoreCp": -200},
                {"move": "h2h4", "scoreCp": -100},
                "g2f3",
            ),
            "none",
        )
        self.assertEqual(
            evidence_class(
                {"mate": None, "scoreCp": 450},
                {"mate": None, "scoreCp": -420},
                {"move": "g2f3", "scoreCp": -300},
                "g2f3",
            ),
            "none",
        )

    def test_sentinel_child_and_policy_are_explicit(self):
        fen = "4r3/2pk2pp/5p2/2P2b2/r7/3n1p2/P2B2PP/R4K1R w - - 0 32"
        self.assertTrue(child_fen(fen, "g2f3").startswith("4r3/2pk2pp/5p2/2P2b2/"))
        raw = {
            "id": "raw",
            "name": "Raw",
            "status": "control",
            "architecture": "raw",
            "extra": ["--no-lmp"],
        }
        sentinel = sentinel_config(raw)
        self.assertIn("--no-null", sentinel["extra"])
        self.assertEqual(
            sentinel["policy"]["authority"],
            "proof-only; cannot choose or reorder live moves",
        )

    def test_sentinel_transition_requires_every_repeat(self):
        rows = [
            {"budgetMs": 10, "verified": True},
            {"budgetMs": 10, "verified": False},
            {"budgetMs": 25, "verified": True},
            {"budgetMs": 25, "verified": True},
        ]
        self.assertEqual(
            all_repeats_transition(rows, [10, 25], "verified"),
            25,
        )

    def test_sentinel_suite_summary_separates_precision_and_recall(self):
        rows = [
            {"evidenceClass": "verified-major-loss", "cpLoss": 500},
            {"evidenceClass": "verified-major-loss", "cpLoss": 20},
            {"evidenceClass": "none", "cpLoss": 400},
            {"evidenceClass": "none", "cpLoss": 0},
        ]
        summary = summarize_sentinel_suite(rows, 300)
        self.assertEqual(summary["truePositives"], 1)
        self.assertEqual(summary["falsePositives"], 1)
        self.assertEqual(summary["falseNegatives"], 1)
        self.assertEqual(summary["trueNegatives"], 1)

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

    def test_result_record_preserves_exact_mate_evidence(self):
        from bench_equal_time import result_record

        record = result_record(
            {
                "uci": "e7e1",
                "scoreCp": 999995,
                "mate": 5,
                "depth": 8,
                "nodes": 100,
                "qNodes": 10,
                "timeMs": 20,
                "pv": ["e7e1"],
                "iterations": [],
                "rootOrder": ["e7e1"],
            }
        )
        self.assertEqual(record["mate"], 5)

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
