"""Negative controls for retaining complete and adverse regression evidence."""
import copy
import importlib.util
import json
from pathlib import Path
import sys
import tempfile
from types import SimpleNamespace
import unittest
from unittest.mock import patch

HERE = Path(__file__).resolve().parent
sys.path.insert(0, str(HERE))
import regression


class RegressionTests(unittest.TestCase):
    @staticmethod
    def small_snapshot(source):
        return {str(path.relative_to(source)): regression.sha(path)
                for path in source.rglob("*") if path.is_file()}

    def test_canonical_staging_preserves_originals_and_baseline_binary(self):
        with tempfile.TemporaryDirectory() as tmp:
            base = Path(tmp)
            sources = {arm: base / arm for arm in regression.ARMS}
            for arm, source in sources.items():
                (source / "src").mkdir(parents=True)
                (source / "Cargo.toml").write_text("identical manifest")
                (source / "src/lib.rs").write_text(arm)
            (sources["candidate"] / "src/extra.rs").write_text("new candidate capability")
            manifests = {arm: self.small_snapshot(source) for arm, source in sources.items()}
            output = base / "evidence"
            context = regression.prepare_canonical(base / "canonical", output, sources)
            with patch("regression.snapshot", side_effect=self.small_snapshot):
                staged = regression.stage_canonical(context, sources["baseline"], manifests["baseline"])
                self.assertEqual(self.small_snapshot(staged), manifests["baseline"])
                common = Path(context["target"]) / "release/ulmdb-engine-benchmark"
                common.parent.mkdir(parents=True)
                common.write_bytes(b"baseline executable")
                preserved = output / "target/baseline/release/ulmdb-engine-benchmark"
                regression.preserve_canonical_binary(common, preserved)
                context["arms"]["baseline"] = {"preserved_binary": str(preserved),
                                                  "preserved_binary_sha256": regression.sha(preserved)}
                candidate_staged = regression.stage_canonical(context, sources["candidate"], manifests["candidate"])
                self.assertEqual(staged, candidate_staged)
                self.assertEqual(self.small_snapshot(candidate_staged), manifests["candidate"])
                self.assertFalse(common.exists(), "shared target must be cleared after preserving baseline")
                common.parent.mkdir(parents=True)
                common.write_bytes(b"candidate executable")
                self.assertEqual(preserved.read_bytes(), b"baseline executable")
                for arm, source in sources.items():
                    self.assertEqual(self.small_snapshot(source), manifests[arm])

    def test_canonical_overlap_ownership_and_input_drift_reject_before_removal(self):
        with tempfile.TemporaryDirectory() as tmp:
            base = Path(tmp)
            source = base / "original"
            source.mkdir()
            (source / "one.rs").write_text("original")
            sources = {arm: source for arm in regression.ARMS}
            output = base / "output"
            with self.assertRaisesRegex(ValueError, "overlap frozen"):
                regression.prepare_canonical(source / "nested", output, sources)
            linked = base / "symlink"
            linked.symlink_to(base / "missing-target", target_is_directory=True)
            with self.assertRaisesRegex(ValueError, "fresh"):
                regression.prepare_canonical(linked, output, sources)
            context = regression.prepare_canonical(base / "canonical", output, sources)
            manifest = self.small_snapshot(source)
            with patch("regression.snapshot", side_effect=self.small_snapshot):
                staged = regression.stage_canonical(context, source, manifest)
                (source / "one.rs").write_text("changed original")
                with self.assertRaisesRegex(ValueError, "frozen source changed"):
                    regression.stage_canonical(context, source, manifest)
                self.assertEqual((staged / "one.rs").read_text(), "original")
                marker = Path(context["directory"]) / ".lookup-regression-owner.json"
                marker.write_text("{}")
                with self.assertRaisesRegex(ValueError, "owner marker changed"):
                    regression.stage_canonical(context, source, self.small_snapshot(source))
                self.assertEqual((staged / "one.rs").read_text(), "original")

    def test_schedule_keeps_adjacent_pairs_and_rotates_order(self):
        jobs = regression.schedule(3)
        self.assertEqual(len(jobs), 12)
        for index in range(0, len(jobs), 2):
            a, b = jobs[index:index + 2]
            self.assertEqual((a["pair"], a["filesystem"]), (b["pair"], b["filesystem"]))
            self.assertEqual({a["arm"], b["arm"]}, {"baseline", "candidate"})
        self.assertEqual({job["arm"] for job in jobs[::2]}, {"baseline", "candidate"})
        self.assertEqual(sum(job["filesystem"] == "ram" for job in jobs), 6)

    def test_relaxed_native_flags_reject_before_timing_data_is_accepted(self):
        bad = {"suite": "full15", "schema": 1, "backend": regression.BACKENDS["resident"],
               "semantic_validation_passed": True, "timing_enabled": True, "native_flags": 1}
        with self.assertRaisesRegex(ValueError, "normal durability"):
            regression.validate_report(bad, b"", "unused", {}, "resident")

    def test_build_only_resume_rejects_source_config_and_partial_campaign_drift(self):
        output = Path("/tmp/regression-resume-negative-control").resolve()
        sources = {arm: output / arm / "source" for arm in regression.ARMS}
        manifests = {arm: {"Cargo.toml": arm} for arm in regression.ARMS}
        args = SimpleNamespace(backend="resident", cpu=20, rounds=3, keep_fixtures=False,
                               ram_dir=Path("/dev/shm"))
        receipt = {"complete": False, "runs": [], "schedule": regression.schedule(3),
                   "configuration": {"build_only": True, "backend": "resident", "cpu": 20,
                                     "rounds": 3, "keep_fixtures": False,
                                     "ram_dir": "/dev/shm", "output": str(output)},
                   "sources": {arm: {"directory": str(sources[arm]), "sha256": manifests[arm]}
                               for arm in regression.ARMS},
                   "binaries": {arm: {"path": str(output / "target" / arm / "release/ulmdb-engine-benchmark")}
                                for arm in regression.ARMS}}
        regression.validate_build_only_resume(receipt, sources, manifests, args, output)
        for change in ("source", "configuration", "partial", "already_resumed"):
            bad = copy.deepcopy(receipt)
            if change == "source":
                bad["sources"]["candidate"]["sha256"]["Cargo.toml"] = "changed"
            elif change == "configuration":
                bad["configuration"]["rounds"] = 4
            elif change == "partial":
                bad["runs"] = [{"completed": "one"}]
            else:
                bad["resumed_from_build_only"] = True
            with self.subTest(change=change), self.assertRaises(ValueError):
                regression.validate_build_only_resume(bad, sources, manifests, args, output)

    def test_real_capture_inventory_and_adverse_case_remain_visible(self):
        original = json.loads((HERE.parent / "results/full.json").read_text())
        baseline = copy.deepcopy(original)
        candidate = copy.deepcopy(original)
        # Deliberately halve one throughput while preserving the local LMDB
        # control. The complete table must expose it, not filter it away.
        for profile in candidate["profiles"]:
            if profile["filesystem"] == "disk":
                for trial in profile["report"]["reads"][0]["trials"]:
                    trial["ulmdb"]["elapsed_ns"] *= 2
        compare = regression.import_compare(HERE.parent / "compare.py")
        result = compare.compare({"full": baseline}, {"full": candidate}, paired=True,
                                 min_repeats=3, adjustment="bonferroni")
        rows = regression.case_table(result)
        self.assertEqual(len(rows), 30)
        target = next(row for row in rows if row["filesystem"] == "disk"
                      and row["case"] == original["profiles"][0]["report"]["reads"][0]["name"])
        self.assertAlmostEqual(target["ulmdb_change_pct"], -50.0)
        self.assertTrue(target["adverse_throughput_over_5_percent"])
        self.assertAlmostEqual(target["lmdb_control_change_pct"], 0.0)
        with tempfile.TemporaryDirectory() as output:
            regression.render(Path(output), {"baseline": baseline, "candidate": candidate}, result)
            text = (Path(output) / "REGRESSION.md").read_text()
            self.assertIn("-50.00%", text)
            self.assertIn("Cold", text.title())
            table = json.loads((Path(output) / "table.json").read_text())
            self.assertEqual(len(table["throughput_cases"]), 30)
            self.assertTrue(table["adverse_target_metrics_over_5_percent"])


if __name__ == "__main__":
    unittest.main()
