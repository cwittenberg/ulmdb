#!/usr/bin/env python3
"""Meaningful harness controls; these do not qualify an LMDB provider."""
from pathlib import Path
import hashlib
import json
import os
import select
import signal
import subprocess
import sys
import tempfile
import time
import types
import unittest

HERE = Path(__file__).resolve().parent
CORE = types.ModuleType('durability_control_core')
CORE.__file__ = str(HERE / 'run.py')
exec(compile((HERE / 'run.py').read_bytes(), str(HERE / 'run.py'), 'exec'), CORE.__dict__)


class HarnessControls(unittest.TestCase):
    def test_partial_ready_line_has_deadline_and_preserves_observed_bytes(self):
        process = subprocess.Popen([sys.executable, '-c', 'import os,time;os.write(1,b"PARTIAL");time.sleep(30)'],
                                   stdout=subprocess.PIPE, stderr=subprocess.PIPE, start_new_session=True)
        try:
            self.assertTrue(select.select([process.stdout], [], [], 5)[0], 'synthetic producer did not start')
            started = time.monotonic()
            with self.assertRaises(TimeoutError) as caught:
                CORE.read_ready(process, b'WEAK_SOURCE_READY\n', 0.25)
            self.assertLess(time.monotonic() - started, 2)
            self.assertEqual(caught.exception.observed_stdout, b'PARTIAL')
        finally:
            if process.poll() is None:
                os.killpg(process.pid, signal.SIGKILL)
            process.communicate(timeout=3)

    def test_owned_deadline_kills_descendant_and_retains_command_evidence(self):
        with tempfile.TemporaryDirectory() as temporary:
            base = Path(temporary)
            run = CORE.Qualification(types.SimpleNamespace(output=base / 'output', timeout=1))
            program = 'import os,time;pid=os.fork();print("child="+str(pid),flush=True);time.sleep(30)'
            with self.assertRaises(RuntimeError):
                run.command('blocked-descendant', [sys.executable, '-c', program])
            rows = [json.loads(line) for line in (run.out / 'commands.jsonl').read_text().splitlines()]
            self.assertEqual(rows[0]['harness_failure'], 'TimeoutExpired')
            stdout = (run.out / rows[0]['stdout']).read_text()
            descendants = [int(line.split('=')[1]) for line in stdout.splitlines() if line.startswith('child=') and int(line.split('=')[1])]
            self.assertEqual(len(descendants), 1)
            status = Path(f'/proc/{descendants[0]}/stat')
            def state():
                try:
                    return status.read_text().split()[2]
                except FileNotFoundError:
                    return None
            deadline = time.monotonic() + 3
            while state() not in {None, 'Z'} and time.monotonic() < deadline:
                time.sleep(0.01)
            self.assertIn(state(), {None, 'Z'}, 'owned descendant is still running')
            receipt = json.loads((run.out / 'receipt.json').read_text())
            self.assertFalse(receipt['complete'])
            self.assertFalse(receipt['passed'])

    def test_contradictory_initializer_build_provenance_is_rejected_before_execution(self):
        with tempfile.TemporaryDirectory() as temporary:
            base = Path(temporary)
            provenance = base / 'provenance.json'
            executable = Path(sys.executable).resolve()
            provenance.write_text(json.dumps({'schema': 1, 'role': 'empty-native-backend-initializer',
                'complete': True, 'passed': True, 'source_before_after_equal': True,
                'entry_source_sha256': CORE.FROZEN['backend_init/main.rs'],
                'executable_sha256': hashlib.sha256(executable.read_bytes()).hexdigest(),
                'commands': [{'label': 'failed-build', 'returncode': 1}],
                'sources': [{'path': str(HERE / 'backend_init/main.rs'), 'sha256': CORE.FROZEN['backend_init/main.rs']}]}))
            run = CORE.Qualification(types.SimpleNamespace(output=base / 'output', timeout=1,
                backend_init=executable, backend_init_provenance=provenance))
            with self.assertRaisesRegex(RuntimeError, 'failed/incomplete build'):
                run.helper_provenance()
            self.assertEqual(run.receipt['command_count'], 0)
            self.assertFalse(run.receipt['passed'])


if __name__ == '__main__':
    unittest.main()
