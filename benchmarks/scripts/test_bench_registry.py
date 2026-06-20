import os
import unittest
from unittest.mock import patch

import benchlib as B


class EngineRegistryTests(unittest.TestCase):
    def setUp(self):
        self.registry = B.load_engine_registry()

    def test_engine_ids_are_unique_and_profiles_exist(self):
        rows = self.registry['engines']
        ids = [row['id'] for row in rows]
        self.assertEqual(len(ids), len(set(ids)))
        for row in rows:
            self.assertIn(row['searchProfile'], self.registry['searchProfiles'])

    def test_stockfish_review_depth_defaults_to_24(self):
        self.assertEqual(B.DEFAULT_STOCKFISH_REVIEW_DEPTH, 24)

    def test_current_defaults_keep_rejected_experiments_off(self):
        options = self.registry['searchProfiles']['current-default-2026-06-19'][
            'effectiveOptions'
        ]
        for key in (
            'lmp',
            'seePrune',
            'deltaPrune',
            'countermove',
            'continuationHistory',
            'captureHistory',
            'tt2',
            'improving',
            'kingActivity',
            'singular',
        ):
            self.assertFalse(options[key], key)

    def test_equal_time_pair_uses_the_same_main_network_and_profile(self):
        raw = B.registered_engine('g9.raw-control.matrix-raw', self.registry)
        hybrid = B.registered_engine(
            'g9.hybrid-a.raw-plus-residual',
            self.registry,
        )
        self.assertEqual(raw['net'], hybrid['net'])
        self.assertEqual(raw['search_profile_sha'], hybrid['search_profile_sha'])
        self.assertIsNone(raw['helper_net'])
        self.assertIsNotNone(hybrid['helper_net'])
        self.assertEqual(
            hybrid['policy']['id'],
            'quiet-root-residual-ordering-v1',
        )

    def test_registry_paths_honor_environment_overrides(self):
        with patch.dict(os.environ, {'CVS_CURRENT_SERVE_EXE': 'custom/analyze.exe'}):
            cfg = B.registered_engine(
                'g9.current-default.raw-plus-residual',
                self.registry,
            )
        self.assertEqual(
            cfg['exe'],
            os.path.normpath(os.path.join(B.REPO, 'custom/analyze.exe')),
        )


if __name__ == '__main__':
    unittest.main()
