#!/usr/bin/env python3
"""Adversarial checks for evidence qualification, using disposable receipts."""
import sys

sys.dont_write_bytecode = True
import argparse
import copy
import json
import shutil
import subprocess
import tempfile
import unittest
from pathlib import Path

import registry

RECEIPT_OVERRIDES = {}


class QualificationControls(unittest.TestCase):
    def setUp(self):
        self.matrix = registry.read_json(registry.DEFAULT_MATRIX)
        receipt_path = RECEIPT_OVERRIDES.get("common-v1", registry.ROOT / self.matrix["receipts"][0]["path"])
        self.receipt = registry.read_json(Path(receipt_path))
        self.scratch = tempfile.TemporaryDirectory(prefix="ulmdb-registry-control-")
        self.addCleanup(self.scratch.cleanup)

    def with_receipt(self, receipt):
        path = Path(self.scratch.name) / "receipt.json"
        path.write_text(json.dumps(receipt))
        return {**RECEIPT_OVERRIDES, "common-v1": str(path)}

    def test_baseline_accepts_visible_known_gaps(self):
        registry.validate(self.matrix, receipt_overrides=RECEIPT_OVERRIDES)
        self.assertEqual(len(self.matrix["features"]), 32)

    def test_missing_export_is_not_hidden_by_passed_receipt(self):
        symbol = self.receipt["common_symbols"]["symbols"][0]
        symbol["exported"] = False
        self.receipt["common_symbols"]["exported"] -= 1
        with self.assertRaisesRegex(registry.Invalid, "missing common export"):
            registry.validate(self.matrix, receipt_overrides=self.with_receipt(self.receipt))

    def test_trace_mismatch_is_not_hidden_by_passed_row(self):
        result = next(row for row in self.receipt["results"] if row["reference_trace_sha256"])
        result["adapter_trace_sha256"] = "0" * 64
        with self.assertRaisesRegex(registry.Invalid, "trace mismatch"):
            registry.validate(self.matrix, receipt_overrides=self.with_receipt(self.receipt))

    def test_recompiled_application_cannot_be_an_unchanged_binary_trace(self):
        result = next(row for row in self.receipt["results"] if row["reference_trace_sha256"])
        result["adapter_executable_sha256"] = "0" * 64
        with self.assertRaisesRegex(registry.Invalid, "changed executable"):
            registry.validate(self.matrix, receipt_overrides=self.with_receipt(self.receipt))

    def test_planned_test_cannot_carry_receipt_evidence(self):
        target = self.matrix["features"][0]["tests"][0]
        target["status"] = "planned"
        target["receipt"] = "common-v1"
        target["selector"] = {"scenario": "Common API", "reference": "0.9.31"}
        with self.assertRaisesRegex(registry.Invalid, "cannot claim recorded evidence"):
            registry.validate(self.matrix, receipt_overrides=RECEIPT_OVERRIDES)

    def test_implemented_source_and_existing_tests_do_not_establish_support(self):
        feature = self.matrix["features"][0]
        feature["state"] = "supported"
        feature["implementation"] = "implemented"
        feature["references"] = ["1.0.2"]
        for test in feature["tests"]:
            test["status"] = "existing"
            test.pop("receipt", None)
            test.pop("selector", None)
        with self.assertRaisesRegex(registry.Invalid, "lacks recorded positive/negative/reopen evidence"):
            registry.validate(self.matrix, receipt_overrides=RECEIPT_OVERRIDES)

    def test_no_recorded_pass_from_a_missing_planned_fixture(self):
        target = self.matrix["features"][0]["tests"][0]
        target.update(path="clients/lmdb-ulmdb/tests/.missing-control-fixture.c",
                      status="recorded", receipt="common-v1",
                      selector={"scenario": "Common API", "reference": "0.9.31"})
        with self.assertRaisesRegex(registry.Invalid, "test path does not exist"):
            registry.validate(self.matrix, receipt_overrides=RECEIPT_OVERRIDES)

    def test_behavioral_traces_do_not_qualify_a_missing_modern_export(self):
        feature = self.matrix["features"][0]
        feature.update(state="supported", implementation="implemented", references=["1.0.2"])
        feature["tests"] = [dict(role=role, case="Disposable export control",
                                 path="clients/lmdb-ulmdb/tests/parity_abi.c", status="recorded",
                                 receipt="common-v1", backends=["adapter-file"],
                                 selector=dict(scenario="Common API", reference="1.0.2"))
                            for role in ("positive", "negative", "reopen")]
        symbols = self.receipt["modern_additional_symbols"]
        symbol = next(row for row in symbols["symbols"] if row["name"] == "mdb_txn_flags")
        if symbol["exported"]:
            symbol["exported"] = False
            symbols["exported"] -= 1
        with self.assertRaisesRegex(registry.Invalid, "missing export mdb_txn_flags"):
            registry.validate(self.matrix, receipt_overrides=self.with_receipt(self.receipt))

    def test_historical_adapter_receipt_does_not_qualify_native_backend(self):
        feature = self.matrix["features"][0]
        feature.update(state="supported", implementation="implemented", references=["0.9.31"],
                       backends=["adapter-file", "resident"])
        # Even passing differential results for the adapter cannot be reassigned to
        # a backend whose methods and storage path were never exercised.
        feature["tests"] = [dict(role=role, case="Disposable evidence scope control",
                                 path="clients/lmdb-ulmdb/tests/parity_abi.c", status="recorded",
                                 receipt="common-v1", backends=["adapter-file"],
                                 selector=dict(scenario="Common API", reference="0.9.31"))
                            for role in ("positive", "negative", "reopen")]
        with self.assertRaisesRegex(registry.Invalid, "no positive evidence for resident"):
            registry.validate(self.matrix, receipt_overrides=RECEIPT_OVERRIDES)

    def test_native_coordination_cannot_remove_its_performance_gate(self):
        feature = next(item for item in self.matrix["features"] if item["id"] == "G05")
        feature["requires_performance"] = False
        with self.assertRaisesRegex(registry.Invalid, "requires explicit matched performance acceptance"):
            registry.validate(self.matrix, receipt_overrides=RECEIPT_OVERRIDES)

    def test_unsupported_modern_exports_remain_visible_and_do_not_fail_inventory(self):
        # Create the gap in this disposable receipt: production may now export
        # every modern symbol. An unqualified extension remains visible without
        # invalidating the common API inventory or becoming qualified support.
        feature = next(row for row in self.matrix["features"] if row["id"] == "F04")
        feature["state"] = "unqualified"
        symbols = self.receipt["modern_additional_symbols"]
        symbol = next(row for row in symbols["symbols"] if row["name"] == "mdb_env_incr_dumpfd")
        if symbol["exported"]:
            symbol["exported"] = False
            symbols["exported"] -= 1
        self.assertFalse(symbol["exported"])
        registry.validate(self.matrix, receipt_overrides=self.with_receipt(self.receipt))


def native_receipt_path():
    matrix = registry.read_json(registry.DEFAULT_MATRIX)
    spec = next(row for row in matrix["receipts"] if row["id"] == "native-coordination")
    return Path(RECEIPT_OVERRIDES.get("native-coordination", registry.ROOT / spec["path"]))


class NativeEvidenceControls(unittest.TestCase):
    """These controls consume a real completed native run, never a fake pass."""
    @classmethod
    def setUpClass(cls):
        cls.original = registry.read_json(native_receipt_path())
        registry.native_evidence.check_native(cls.original)

    def setUp(self):
        self.native = copy.deepcopy(self.original)

    def check_invalid(self, message):
        with self.assertRaisesRegex(registry.native_evidence.Invalid, message):
            registry.native_evidence.check_native(self.native)

    def test_native_wrong_backend_cannot_inherit_coverage(self):
        self.native["results"][0]["backend"] = "resident"
        self.check_invalid("wrong backend/native test result")

    def test_native_source_hash_must_match_actual_current_inputs(self):
        self.native["source"]["files"][0]["sha256"] = "0" * 64
        self.native["source"]["sha256"] = registry.native_evidence.aggregate(self.native["source"]["files"])
        self.check_invalid("source hashes differ")

    def test_native_missing_case_cannot_keep_complete_pass(self):
        self.native["results"].pop()
        self.check_invalid("missing or wrong backend/native test result")

    def test_native_failed_run_cannot_keep_top_level_pass(self):
        self.native["results"][0]["run"].update(exit_code=7, passed=False)
        self.check_invalid("native command failed")

    def test_native_result_executable_must_match_frozen_suite(self):
        self.native["results"][0]["executable_sha256"] = "0" * 64
        self.check_invalid("source/executable binding mismatch")

    def test_native_stdout_cannot_be_edited_without_matching_log_hash(self):
        self.native["results"][0]["run"]["stdout"]["text"] += "CASE_PASS test=invented backend=resident\n"
        self.check_invalid("log hash mismatch")

    def test_native_ignored_test_does_not_qualify(self):
        output = "test result: ok. 0 passed; 0 failed; 1 ignored; 0 measured; 0 filtered out\n"
        log = self.native["results"][0]["run"]["stdout"]
        log.update(path="target/native-negative-controls/nonexistent-ignored.log", text=output,
                   bytes=len(output.encode()), sha256=registry.native_evidence.text_digest(output))
        self.check_invalid("test ignored, missing or failed")

    def test_native_self_oracle_cannot_qualify_public_extension_function(self):
        row = next(row for row in self.native["results"] if "positive" in row["roles"])
        suite = next(suite for suite in self.native["suites"] if suite["id"] == row["suite"])
        test = dict(role="positive", path=suite["source"], backends=[row["backend"]],
                    receipt="native", selector={key: row[key] for key in ("suite", "test", "backend")})
        with self.assertRaisesRegex(registry.Invalid, "cannot qualify public LMDB APIs"):
            registry.result_for(test, {"native": self.native}, "F01")

    def test_native_receipt_validates_in_an_identical_relocated_checkout_without_artifacts(self):
        with tempfile.TemporaryDirectory(prefix="ulmdb-native-relocation-") as directory:
            relocated = Path(directory)
            subprocess.run(["git", "init", "-q", str(relocated)], check=True)
            for item in self.native["source"]["files"]:
                destination = relocated / item["path"]
                destination.parent.mkdir(parents=True, exist_ok=True)
                shutil.copyfile(registry.ROOT / item["path"], destination)
            self.assertFalse((relocated / "target").exists())
            self.assertNotEqual(str(relocated), self.native["capture_root"])
            registry.native_evidence.check_native(self.native, root=relocated)
            # Capture-root artifacts still exist, but required mode must demand
            # files from this checkout, and optional mode must check local files.
            with self.assertRaisesRegex(registry.native_evidence.Invalid, "frozen native executable mismatch"):
                registry.native_evidence.check_native(self.native, root=relocated, require_artifacts=True)
            local_adapter = relocated / self.native["adapter_input"]["path"]
            local_adapter.parent.mkdir(parents=True, exist_ok=True)
            local_adapter.write_bytes(b"damaged local artifact")
            with self.assertRaisesRegex(registry.native_evidence.Invalid, "frozen native executable mismatch"):
                registry.native_evidence.check_native(self.native, root=relocated)
            local_adapter.unlink()
            local_adapter.symlink_to(registry.ROOT / self.native["adapter_input"]["path"])
            with self.assertRaisesRegex(registry.native_evidence.Invalid, "inside repository target"):
                registry.native_evidence.check_native(self.native, root=relocated)

    def test_native_capture_root_must_be_absolute_and_canonical(self):
        for value in (None, "", 17, "relative/root", "/root/../other", "/root/./other",
                      "/root//other", "//root/other", "/root/\0other"):
            with self.subTest(root=value):
                self.native["capture_root"] = value
                self.check_invalid("invalid captured workspace root")

    def test_native_capture_root_cannot_disagree_with_recorded_commands(self):
        self.native["capture_root"] = "/different-captured-workspace"
        self.check_invalid("unexpected test-list command")

    def test_native_list_argv_cannot_use_another_workspace(self):
        self.native["suites"][0]["list"]["command"][0] = "/different-captured-workspace/target/test"
        self.check_invalid("unexpected test-list command")

    def test_native_run_argv_cannot_use_another_workspace(self):
        self.native["results"][0]["run"]["command"][0] = "/different-captured-workspace/target/test"
        self.check_invalid("unexpected native test command")

    def test_native_mixed_environment_cannot_use_another_workspace(self):
        row = next(row for row in self.native["results"] if row["suite"] == "mixed-adapter")
        row["run"]["environment"]["ULMDB_COORDINATION_DYLIB"] = "/different-captured-workspace/target/adapter.so"
        self.check_invalid("actual shared-library input not selected")

    def test_native_provider_marker_cannot_name_another_library(self):
        row = next(row for row in self.native["results"] if row["suite"] == "mixed-adapter")
        log = row["run"]["stdout"]
        value = log["text"].replace("mixed-parent-provider=", "mixed-parent-provider=/different-library")
        log.update(path="target/native-negative-controls/nonexistent-provider.log", text=value,
                   bytes=len(value.encode()), sha256=registry.native_evidence.text_digest(value))
        self.check_invalid("missing actual mixed shared-library provider marker")

    def test_native_artifact_path_cannot_traverse_out_of_current_workspace(self):
        self.native["suites"][0]["executable"]["path"] = "target/../outside/native"
        self.check_invalid("invalid artifact path")


def load_tests(loader, tests, pattern):
    suite = unittest.TestSuite([loader.loadTestsFromTestCase(QualificationControls)])
    # Optional evidence is absent in an ordinary source inventory checkout. The
    # native qualification gate supplies its fresh receipt, making every control
    # mandatory there. No synthetic native success or skipped test is generated.
    if native_receipt_path().is_file():
        suite.addTests(loader.loadTestsFromTestCase(NativeEvidenceControls))
    return suite


if __name__ == "__main__":
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--receipt", action="append", default=[], metavar="ALIAS=PATH")
    args, remaining = parser.parse_known_args()
    RECEIPT_OVERRIDES.update(item.split("=", 1) for item in args.receipt)
    unittest.main(argv=[sys.argv[0], *remaining])
