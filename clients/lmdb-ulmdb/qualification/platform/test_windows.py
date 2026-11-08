#!/usr/bin/env python3
"""Small admission/ownership controls for the native Windows host entrypoint."""
import importlib.util
import json
import os
from pathlib import Path
import subprocess
import sys
import tempfile
import time
import unittest
from unittest.mock import patch

spec = importlib.util.spec_from_file_location("windows_host_source", Path(__file__).with_name("windows.py"))
host = importlib.util.module_from_spec(spec)
# Source compilation avoids accepting a stale bytecode cache as the controller.
exec(compile(Path(spec.origin).read_bytes(), spec.origin, "exec"), host.__dict__)


class Guards(unittest.TestCase):
    def test_hash_guard_rejects_mutation_and_size(self):
        with tempfile.TemporaryDirectory() as tmp:
            p = Path(tmp) / "source"
            p.write_bytes(b"approved")
            expected = host.sha(p)
            host.checked_file(p, expected, 8)
            with self.assertRaises(RuntimeError):
                host.checked_file(p, expected, 7)
            p.write_bytes(b"mutation")
            with self.assertRaises(RuntimeError):
                host.checked_file(p, expected, 8)

    def test_exact_test_guard_rejects_empty_or_wrong_tests(self):
        good = b"test wanted ... ok\ntest result: ok. 1 passed; 0 failed; 0 ignored;\n"
        host.rust_pass(good, "wanted")
        for bad in (b"test result: ok. 0 passed; 0 failed;", good.replace(b"wanted", b"other"),
                    good.replace(b"1 passed", b"2 passed"), good.replace(b"0 failed", b"1 failed"),
                    good.replace(b"... ok", b"... FAILED")):
            with self.assertRaises(RuntimeError):
                host.rust_pass(bad, "wanted")
        # A parent can incorrectly accept its exec worker's zero-test success.
        # Every nested summary must represent an actual successful body too.
        host.rust_pass(b"test result: ok. 2 passed; 0 failed; 0 ignored;\n" + good, "wanted")
        for child in (b"test result: ok. 0 passed; 0 failed; 0 ignored;\n",
                      b"test result: ok. 0 passed; 0 failed; 1 ignored;\n",
                      b"test result: FAILED. 0 passed; 1 failed; 0 ignored;\n"):
            with self.assertRaises(RuntimeError):
                host.rust_pass(child + good, "wanted")

    def test_exit_guard_rejects_other_nonzero_and_crash_status(self):
        host.exit_pass(1, 1)
        for actual in (0, 2, -9, 0xc0000005, -1073741819):
            with self.assertRaises(RuntimeError):
                host.exit_pass(actual, 1)

    def test_pe_guard_rejects_wrong_crt_and_provider(self):
        good = b"DLL Name: msvcrt.dll\nDLL Name: ulmdb_c.dll\n"
        host.imports(good, provider=True)
        for bad in (good.replace(b"msvcrt", b"ucrtbase"), good + b"DLL Name: api-ms-win-crt-stdio-l1-1-0.dll\n",
                    good.replace(b"ulmdb_c", b"lmdb"), good + b"DLL Name: lmdb.dll\n"):
            with self.assertRaises(RuntimeError):
                host.imports(bad, provider=True)

    def test_windows_abi_and_host_rejection(self):
        self.assertEqual(host.C.sizeof(host.BasicLimits), 64)
        self.assertEqual(host.C.sizeof(host.ExtendedLimits), 144)
        if os.name != "nt":
            with self.assertRaisesRegex(RuntimeError, "genuine native Windows"):
                host.require_windows()

    def test_nonzero_and_start_failure_keep_logs_and_rows(self):
        events = []
        class Job:
            def attach_and_resume(self, process):
                events.append("attach")
            def close(self):
                events.append("close")
        real = subprocess.Popen
        def spawn(*args, **kwargs):
            self.assertEqual(kwargs.pop("creationflags"), host.SUSPENDED_GROUP)
            return real(*args, **kwargs)
        with tempfile.TemporaryDirectory() as tmp, patch.object(host.subprocess, "Popen", side_effect=spawn):
            run = host.Run(Path(tmp) / "run", job_factory=Job)
            with self.assertRaisesRegex(RuntimeError, "unexpected exit 7"):
                run.command("nonzero", [sys.executable, "-c", "import sys;print('partial');print('error',file=sys.stderr);sys.exit(7)"])
            self.assertEqual((run.out / "nonzero.stdout").read_bytes().strip(), b"partial")
            self.assertEqual(events, ["attach", "close"])
            with self.assertRaisesRegex(RuntimeError, "unexpected exit 2; expected 1"):
                run.command("wrong-nonzero", [sys.executable, "-c", "raise SystemExit(2)"], expected=1)
            with self.assertRaises(FileNotFoundError):
                run.command("not-found", [str(Path(tmp) / "absent-program")])
            receipt = json.loads((run.out / "receipt.json").read_text())
            self.assertEqual(receipt["commands"][0]["exit_code"], 7)
            self.assertTrue(receipt["commands"][0]["terminal"])
            self.assertEqual(receipt["commands"][1]["exit_code"], 2)
            self.assertIn("error", receipt["commands"][2])
            self.assertFalse(receipt["passed"])

    def test_timeout_preserves_partial_stream_and_terminates_child(self):
        # Real direct process on Linux, genuine Job ownership on Windows.
        class DirectJob:
            def attach_and_resume(self, process):
                pass
            def close(self):
                pass
        real = subprocess.Popen
        def spawn(*args, **kwargs):
            kwargs.pop("creationflags")
            return real(*args, **kwargs)
        with tempfile.TemporaryDirectory() as tmp:
            run = host.Run(Path(tmp) / "run", job_factory=host.Job if os.name == "nt" else DirectJob)
            context = patch.object(host.subprocess, "Popen", side_effect=spawn) if os.name != "nt" else patch.dict(os.environ, {})
            with context, self.assertRaises(subprocess.TimeoutExpired):
                run.command("deadline", [sys.executable, "-u", "-c", "import time;print('before-deadline');time.sleep(20)"], timeout=0.2)
            self.assertIn(b"before-deadline", (run.out / "deadline.stdout").read_bytes())
            row = run.receipt["commands"][0]
            self.assertTrue(row["terminal"])
            self.assertNotEqual(row["exit_code"], 0)
            self.assertIn("TimeoutExpired", row["error"])

    @unittest.skipUnless(os.name == "nt", "native Windows Job control")
    def test_native_job_stops_descendant_after_leader_exits(self):
        with tempfile.TemporaryDirectory() as tmp:
            run = host.Run(Path(tmp) / "run")
            marker = Path(tmp) / "escaped"
            child = "import pathlib,time;time.sleep(1);pathlib.Path(" + repr(str(marker)) + ").write_text('escaped')"
            parent = "import subprocess,sys;subprocess.Popen([sys.executable,'-c'," + repr(child) + "]);print('leader-complete')"
            self.assertIn(b"leader-complete", run.command("descendant", [sys.executable, "-u", "-c", parent]))
            time.sleep(1.3)
            self.assertFalse(marker.exists(), "descendant escaped the owned kill-on-close Job")


if __name__ == "__main__":
    unittest.main()
