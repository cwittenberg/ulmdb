#!/usr/bin/env python3
"""Exercise cancellation against real owned subprocesses, without creating receipts."""
import json
import os
from pathlib import Path
import signal
import subprocess
import sys
import tempfile
import time
import unittest

sys.dont_write_bytecode = True
import run_native


CHILD = """
import os, pathlib, signal, time
signal.signal(signal.SIGTERM, signal.SIG_IGN)
pathlib.Path({path!r}).write_text(str(os.getpid()))
while True: time.sleep(1)
"""


def parent_program(path):
    child = CHILD.format(path=str(path))
    return ("import subprocess,sys,time; "
            f"subprocess.Popen([sys.executable,'-c',{child!r}], stdout=subprocess.DEVNULL, stderr=subprocess.DEVNULL); "
            "print('owned-parent-output',flush=True); time.sleep(120)")


class OwnedProcessControls(unittest.TestCase):
    def setUp(self):
        self.scratch = tempfile.TemporaryDirectory(prefix="ulmdb-native-runner-control-")
        self.addCleanup(self.scratch.cleanup)
        self.pid_path = Path(self.scratch.name) / "worker.pid"

    def assert_worker_stopped(self):
        pid = int(self.pid_path.read_text())
        deadline = time.monotonic() + 2
        while time.monotonic() < deadline:
            try:
                state = Path(f"/proc/{pid}/stat").read_text().rsplit(")", 1)[1].split()[0]
            except FileNotFoundError:
                return
            if state == "Z":
                return
            time.sleep(0.01)
        self.fail(f"owned worker {pid} remains alive after cancellation")

    def test_timeout_kills_worker_even_after_parent_and_captured_pipes_exit(self):
        stdout, _, code, reason = run_native.communicate_owned(
            [sys.executable, "-c", parent_program(self.pid_path)], os.environ.copy(), 0.5)
        self.assertEqual((code, reason), (-1, "timeout"))
        self.assertIn("owned-parent-output", stdout)
        self.assert_worker_stopped()

    def test_keyboard_interrupt_kills_owned_workers_and_retains_partial_output(self):
        code = (f"import sys; sys.path.insert(0,{str(Path(run_native.__file__).parent)!r}); "
                "import run_native,os,json; "
                f"r=run_native.communicate_owned([sys.executable,'-c',{parent_program(self.pid_path)!r}],os.environ.copy(),30); "
                "print(json.dumps(r))")
        driver = subprocess.Popen([sys.executable, "-c", code], stdout=subprocess.PIPE,
                                  stderr=subprocess.PIPE, text=True)
        try:
            deadline = time.monotonic() + 5
            while not self.pid_path.exists() and time.monotonic() < deadline:
                self.assertIsNone(driver.poll())
                time.sleep(0.01)
            self.assertTrue(self.pid_path.exists(), "owned worker failed to start")
            driver.send_signal(signal.SIGINT)
            stdout, stderr = driver.communicate(timeout=5)
            self.assertEqual(driver.returncode, 0, stderr)
            output, _, code, reason = json.loads(stdout)
            self.assertEqual((code, reason), (-1, "interrupted"))
            self.assertIn("owned-parent-output", output)
            self.assert_worker_stopped()
        finally:
            if driver.poll() is None:
                driver.kill()
                driver.communicate()


if __name__ == "__main__":
    unittest.main()
