"""Confirmation guards never permit different inputs or replace primary evidence."""
import copy
import json
from pathlib import Path
import sys
import tempfile
from types import SimpleNamespace
import unittest
from unittest.mock import patch

HERE = Path(__file__).resolve().parent
sys.path.insert(0, str(HERE))
import confirmation


class ConfirmationTests(unittest.TestCase):
    def receipt(self):
        schedule = confirmation.reg.schedule(3)
        return {"complete": True, "backend": "resident",
                "configuration": {"cpu": 12, "rounds": 3, "backend": "resident"},
                "sources": {arm: {"directory": "/frozen/" + arm, "sha256": {"one": "hash"}}
                            for arm in confirmation.reg.ARMS},
                "binaries": {arm: {"path": "/built/" + arm, "sha256": "hash"}
                             for arm in confirmation.reg.ARMS},
                "schedule": schedule, "runs": copy.deepcopy(schedule)}

    def test_configuration_and_process_inventory_drift_reject(self):
        primary = self.receipt()
        args = SimpleNamespace(cpu=12, rounds=9)
        with patch("confirmation.os.sched_getaffinity", return_value={12}):
            confirmation.check_configuration(primary, args)
            for change in ("cpu", "backend", "missing_process", "order", "incomplete"):
                bad = copy.deepcopy(primary)
                if change == "cpu": bad["configuration"]["cpu"] = 13
                elif change == "backend": bad["backend"] = "default"
                elif change == "missing_process": bad["runs"].pop()
                elif change == "order": bad["runs"][0], bad["runs"][1] = bad["runs"][1], bad["runs"][0]
                else: bad["complete"] = False
                with self.subTest(change=change), self.assertRaises(ValueError):
                    confirmation.check_configuration(bad, args)

    def test_source_and_binary_drift_reject_without_any_process_launch(self):
        primary = self.receipt()
        with patch("confirmation.reg.verify_source", side_effect=ValueError("frozen source changed")):
            with self.assertRaisesRegex(ValueError, "frozen source changed"):
                confirmation.check_inputs(primary)
        with tempfile.TemporaryDirectory() as tmp:
            binary = Path(tmp) / "engine"
            binary.write_bytes(b"original")
            for arm in confirmation.reg.ARMS:
                primary["binaries"][arm] = {"path": str(binary), "sha256": confirmation.sha(binary)}
            with patch("confirmation.reg.verify_source"):
                confirmation.check_inputs(primary)
                binary.write_bytes(b"changed")
                with self.assertRaisesRegex(ValueError, "binary changed"):
                    confirmation.check_inputs(primary)

    def test_primary_artifact_mutation_rejects(self):
        with tempfile.TemporaryDirectory() as tmp:
            path = Path(tmp) / "primary.json"
            path.write_text("original")
            expected = {"primary.json": confirmation.sha(path)}
            confirmation.verify_primary_files(Path(tmp), expected)
            path.write_text("changed")
            with self.assertRaisesRegex(ValueError, "primary artifact changed"):
                confirmation.verify_primary_files(Path(tmp), expected)

    def test_delta_keeps_all_primary_and_confirmation_rows_separate(self):
        before = []
        for medium in ("disk", "ram"):
            for index in range(15):
                before.append({"filesystem": medium, "case": str(index), "ulmdb_change_pct": -6.0,
                               "sampled_p99_change_pct": 7.0, "lmdb_control_change_pct": -2.0})
        after = copy.deepcopy(before)
        for row in after: row["ulmdb_change_pct"] = -1.0
        with tempfile.TemporaryDirectory() as tmp:
            confirmation.primary_delta(Path(tmp), {"throughput_cases": before}, {"throughput_cases": after})
            result = json.loads((Path(tmp) / "PRIMARY_VS_CONFIRMATION.json").read_text())
            self.assertEqual(len(result["rows"]), 30)
            self.assertEqual(result["rows"][0]["primary"]["ulmdb_change_pct"], -6.0)
            self.assertEqual(result["rows"][0]["confirmation"]["ulmdb_change_pct"], -1.0)
            self.assertEqual(result["rows"][0]["throughput_change_delta_percentage_points"], 5.0)
        self.assertEqual(before[0]["ulmdb_change_pct"], -6.0)


if __name__ == "__main__":
    unittest.main()
