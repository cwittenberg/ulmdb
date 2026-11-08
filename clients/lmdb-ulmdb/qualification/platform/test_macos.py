"""Local runner controls; none of these is a macOS engine runtime result."""
import importlib.util
import json
from pathlib import Path
import os
import subprocess
import sys
import tempfile
import unittest

SCRIPT = Path(__file__).with_name("macos.py")
SPEC = importlib.util.spec_from_file_location("macos_job", SCRIPT)
macos = importlib.util.module_from_spec(SPEC)
SPEC.loader.exec_module(macos)


class Controls(unittest.TestCase):
    @unittest.skipIf(sys.platform == "darwin", "requires an actual wrong host")
    def test_wrong_host_writes_failed_receipt_without_running_builds(self):
        with tempfile.TemporaryDirectory() as temp:
            output = Path(temp) / "wrong-host"
            child = subprocess.run([sys.executable, SCRIPT, "--output", output],
                                   capture_output=True, text=True, timeout=15)
            self.assertNotEqual(child.returncode, 0)
            record = json.loads((output / "receipt.json").read_text())
            self.assertTrue(record["complete"])
            self.assertFalse(record["passed"])
            self.assertFalse(record["host_admitted"])
            self.assertEqual(record["commands"], [])

    def test_missing_ignored_and_zero_tests_never_count_as_runtime(self):
        good = "test required ... ok\ntest result: ok. 1 passed; 0 failed; 0 ignored;"
        for text in ("", "test result: ok. 0 passed; 0 failed; 0 ignored;",
                     "test required ... ignored\ntest result: ok. 1 passed; 0 failed; 1 ignored;",
                     "test other ... ok\ntest result: ok. 1 passed; 0 failed; 0 ignored;",
                     "test result: ok. 0 passed; 0 failed; 0 ignored;\n" + good,
                     "test result: ok. 1 passed; 0 failed; 1 ignored;\n" + good,
                     "test result: FAILED. 0 passed; 1 failed; 0 ignored;\n" + good,
                     good + "\ntest result: ok. 2 passed; 0 failed; 0 ignored;"):
            with self.subTest(text=text), self.assertRaises(RuntimeError):
                macos.require_executed_tests(text, ["required"])
        macos.require_executed_tests(good, ["required"])
        macos.require_executed_tests("test worker ... ok\ntest result: ok. 1 passed; 0 failed; 0 ignored;\n"
                                     + good, ["required"])

    def test_cargo_build_scripts_are_not_mistaken_for_test_executables(self):
        messages = [{"reason": "compiler-artifact", "profile": {"test": False},
                     "target": {"name": "build-script-build"}, "executable": "/wrong"},
                    {"reason": "compiler-artifact", "profile": {"test": True},
                     "target": {"name": "actual"}, "executable": "/actual"}]
        text = "\n".join(json.dumps(item) for item in messages)
        self.assertEqual(macos.test_executables(text, ["actual"]), {"actual": Path("/actual")})
        with self.assertRaises(RuntimeError):
            macos.test_executables(text, ["missing"])

    @unittest.skipUnless(hasattr(os, "waitid"), "needs native WNOWAIT")
    def test_success_reaps_the_owned_leader_and_binds_the_log(self):
        with tempfile.TemporaryDirectory() as temp:
            run = macos.Run(Path(temp) / "success", 10)
            self.assertIn("owned-success", run.command("success", [sys.executable, "-c", "print('owned-success')"]))
            record = run.receipt["commands"][0]
            self.assertTrue(record["terminal"])
            self.assertEqual(record["remaining_after_cleanup"], [])
            self.assertEqual(record["log_sha256"], macos.digest(record["log"]))
            with self.assertRaises(ChildProcessError):
                os.waitpid(record["pid"], os.WNOHANG)

    @unittest.skipUnless(hasattr(os, "waitid"), "needs native WNOWAIT")
    def test_deadline_kills_the_actual_owned_group_before_reaping(self):
        with tempfile.TemporaryDirectory() as temp:
            run = macos.Run(Path(temp) / "timeout", 2)
            body = ("import subprocess,sys,time; "
                    "p=subprocess.Popen([sys.executable,'-c','import time;time.sleep(30)']); "
                    "print(p.pid,flush=True); time.sleep(30)")
            with self.assertRaisesRegex(RuntimeError, "stage deadline"):
                run.command("timeout", [sys.executable, "-c", body])
            record = run.receipt["commands"][0]
            self.assertTrue(record["terminal"])
            self.assertGreaterEqual(len(record["remaining_before_cleanup"]), 2)
            self.assertEqual(record["remaining_after_cleanup"], [])
            self.assertLess(record["seconds"], 10)
            self.assertFalse(run.receipt["passed"])
            with self.assertRaises(ChildProcessError):
                os.waitpid(record["pid"], os.WNOHANG)


if __name__ == "__main__":
    unittest.main()
