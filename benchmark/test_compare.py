"""Focused checks for comparison math and acceptance of real benchmark captures."""
import copy
import importlib.util
import json
import math
from pathlib import Path
import unittest

HERE = Path(__file__).resolve().parent
SPEC = importlib.util.spec_from_file_location('benchmark_compare', HERE / 'compare.py')
compare = importlib.util.module_from_spec(SPEC)
SPEC.loader.exec_module(compare)


class StatisticsTests(unittest.TestCase):
    def test_student_critical_known_values_and_extreme_familywise_tail(self):
        for degrees, expected in [(1, 12.706204736), (2, 4.30265273),
                                  (10, 2.228138852), (30, 2.042272456)]:
            self.assertAlmostEqual(compare.student_critical(.05, degrees), expected, places=7)
        # df=1 is Cauchy, whose exact inverse remains available at tiny tails.
        self.assertAlmostEqual(compare.student_critical(.0001, 1),
                               1 / math.tan(math.pi * .0001 / 2), places=6)

    def test_paired_interval_preserves_pair_information(self):
        baseline = [1., 10., 100., 1000.]
        candidate = [2., 20., 200., 2000.]
        logarithmic, center, bounds, degrees = compare.interval(baseline, candidate, True, .05)
        self.assertTrue(logarithmic)
        self.assertAlmostEqual(math.exp(center), 2.)
        self.assertAlmostEqual(bounds[0], bounds[1])
        self.assertEqual(degrees, 3)
        independent = compare.interval(baseline, candidate, False, .05)
        self.assertGreater(independent[2][1] - independent[2][0], 1.)

    def test_one_run_has_no_interval_and_zero_values_use_differences(self):
        self.assertIsNone(compare.interval([10.], [11.], False, .05)[2])
        with self.assertRaises(ValueError):
            compare.interval([10., 11.], [12.], True, .05)
        logarithmic, center, bounds, _ = compare.interval([0., 0.], [2., 2.], True, .05)
        self.assertFalse(logarithmic)
        self.assertEqual(center, 2.)
        self.assertEqual(bounds, [2., 2.])


class RealCaptureTests(unittest.TestCase):
    @classmethod
    def setUpClass(cls):
        cls.full = json.loads((HERE / 'results/full.json').read_text())
        cls.millions = json.loads((HERE / 'results/millions.json').read_text())

    def test_real_full_and_million_inventories_and_self_comparison(self):
        result = compare.compare({'full': self.full, 'millions': self.millions},
                                 {'full': self.full, 'millions': self.millions}, paired=True,
                                 min_repeats=3)
        self.assertTrue(result['summary']['screen_clear'])
        ids = {r['id'] for r in result['rows']}
        self.assertIn('full/ram/ulmdb/durable_batch1/throughput', ids)
        self.assertIn('full/disk/ulmdb/ipv6_lpm_inside_gap_misses/sampled_p99', ids)
        self.assertIn('millions/disk/ulmdb/close_reopen_ns', ids)
        self.assertIn('millions/disk/ulmdb/storage_after_close_reopen/allocated_bytes', ids)
        self.assertFalse(result['summary']['adverse_point_estimate'])
        self.assertIn('no equivalence', result['method']['scope'])

    def test_known_write_slowdown_and_reopen_increase_are_never_hidden(self):
        candidate = copy.deepcopy(self.full)
        for profile in candidate['profiles']:
            for trial in profile['report']['writes'][0]['trials']:
                trial['ulmdb']['aggregate_elapsed_ns'] *= 2
            profile['report']['cold']['open_ns'] *= 3
        result = compare.compare({'full': self.full}, {'full': candidate}, paired=True)
        rows = {r['id']: r for r in result['rows']}
        row = rows['full/ram/ulmdb/durable_batch1/throughput']
        self.assertAlmostEqual(row['ratio'], .5)
        self.assertTrue(row['adverse_point_estimate'])
        self.assertIn('adverse_point_estimate', row['flags'])
        self.assertTrue(rows['full/disk/ulmdb/reopen/open_ns']['adverse_point_estimate'])
        self.assertFalse(result['summary']['screen_clear'])

    def test_changed_workload_failed_oracle_and_missing_process_are_rejected(self):
        for change in ['workload', 'validation', 'missing']:
            candidate = copy.deepcopy(self.millions)
            if change == 'workload':
                for profile in candidate['profiles']:
                    profile['report']['workload']['read_checksum'] = 'changed'
            elif change == 'validation':
                candidate['profiles'][0]['report']['validation']['passed'] = False
            else:
                candidate['profiles'].pop()
            with self.subTest(change=change), self.assertRaises(ValueError):
                compare.compare({'millions': self.millions}, {'millions': candidate})

    def test_insufficient_repeats_does_not_pass_even_with_identical_results(self):
        result = compare.compare({'millions': self.millions}, {'millions': self.millions},
                                 paired=True, min_repeats=self.millions['rounds'] + 1)
        self.assertFalse(result['summary']['screen_clear'])
        self.assertTrue(result['summary']['insufficient_repeats'])


if __name__ == '__main__':
    unittest.main()
