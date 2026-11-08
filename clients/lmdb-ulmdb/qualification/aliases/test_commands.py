#!/usr/bin/env python3
"""Actual failed-command evidence and ownership controls; no provider claim."""
import hashlib
import json
import os
from pathlib import Path
import select
import signal
import subprocess
import sys
import tempfile
import time
import unittest
from unittest import mock

import commands
import processes


class Owner:
    def __init__(self, output):
        self.output = output
        output.mkdir()
        self.receipt = {'commands': [], 'complete': False, 'passed': False}

    def save(self):
        (self.output / 'receipt.json').write_text(json.dumps(self.receipt, indent=2) + '\n')


def state(pid):
    try:
        return Path(f'/proc/{pid}/stat').read_text().rsplit(')', 1)[1].split()[0]
    except FileNotFoundError:
        return None


class CommandControls(unittest.TestCase):
    def tearDown(self):
        self.assertFalse(processes.ACTIVE, 'owned command registry leaked')

    def evidence(self, owner):
        receipt = json.loads((owner.output / 'receipt.json').read_text())
        self.assertFalse(receipt['complete'])
        self.assertFalse(receipt['passed'])
        self.assertEqual(len(receipt['commands']), 1)
        row = receipt['commands'][0]
        for label in ['stdout', 'stderr']:
            data = (owner.output / row[label]).read_bytes()
            self.assertEqual(hashlib.sha256(data).hexdigest(), row[label + '_sha256'])
        return row

    def test_nonzero_exit_preserves_both_streams_and_actual_code(self):
        with tempfile.TemporaryDirectory() as temporary:
            owner = Owner(Path(temporary) / 'output')
            with self.assertRaises(RuntimeError):
                commands.run(owner, 'exit-7', [sys.executable, '-c',
                    'import os;os.write(1,b"actual stdout\\n");os.write(2,b"actual stderr\\n");raise SystemExit(7)'])
            row = self.evidence(owner)
            self.assertEqual(row['returncode'], 7)
            self.assertEqual(row['unexpected_returncode'], {'expected': 0, 'actual': 7})
            self.assertEqual((owner.output / row['stdout']).read_bytes(), b'actual stdout\n')
            self.assertEqual((owner.output / row['stderr']).read_bytes(), b'actual stderr\n')
            self.assertFalse(row['deadline'])

    def test_start_failure_is_recorded_without_a_fabricated_child(self):
        with tempfile.TemporaryDirectory() as temporary:
            owner = Owner(Path(temporary) / 'output')
            with self.assertRaises(FileNotFoundError):
                commands.run(owner, 'missing-executable', [str(Path(temporary) / 'does-not-exist')])
            row = self.evidence(owner)
            self.assertIsNone(row['pid'])
            self.assertIsNone(row['returncode'])
            self.assertEqual(row['harness_failure']['type'], 'FileNotFoundError')
            self.assertFalse(row['deadline'])

    def test_deadline_keeps_partial_bytes_and_stops_term_ignoring_descendant(self):
        with tempfile.TemporaryDirectory() as temporary:
            owner = Owner(Path(temporary) / 'output')
            program = ('import os,signal,time;pid=os.fork();'
                'signal.signal(signal.SIGTERM,signal.SIG_IGN) if pid==0 else None;'
                'print("child="+str(pid),flush=True);os.write(2,b"before deadline\\n");time.sleep(30)')
            started = time.monotonic()
            with self.assertRaises(subprocess.TimeoutExpired):
                commands.run(owner, 'deadline', [sys.executable, '-c', program], timeout=0.3)
            self.assertLess(time.monotonic() - started, 5)
            row = self.evidence(owner)
            self.assertTrue(row['deadline'])
            self.assertEqual(row['harness_failure']['type'], 'TimeoutExpired')
            output = (owner.output / row['stdout']).read_text()
            children = [int(line.split('=')[1]) for line in output.splitlines()
                        if line.startswith('child=') and int(line.split('=')[1])]
            self.assertEqual(len(children), 1)
            self.assertIn(b'before deadline\n', (owner.output / row['stderr']).read_bytes())
            deadline = time.monotonic() + 2
            while state(children[0]) not in {None, 'Z', 'X'} and time.monotonic() < deadline:
                time.sleep(0.01)
            self.assertIn(state(children[0]), {None, 'Z', 'X'})

    def test_real_sigterm_records_partial_child_output_before_reraising(self):
        with tempfile.TemporaryDirectory() as temporary:
            base = Path(temporary)
            script = base / 'driver.py'
            here = Path(__file__).resolve().parent
            script.write_text('import sys,json\nfrom pathlib import Path\n'
                + 'sys.path.insert(0,' + repr(str(here)) + ')\nimport commands,processes\n'
                + 'class Owner:\n def __init__(self):\n  self.output=Path(' + repr(str(base / 'output')) + ')\n  self.output.mkdir()\n  self.receipt={"commands":[],"complete":False,"passed":False}\n'
                + ' def save(self):\n  (self.output/"receipt.json").write_text(json.dumps(self.receipt))\n'
                + 'owner=Owner()\nprocesses.install_handlers()\ntry:\n commands.run(owner,"interrupted",[sys.executable,"-c",'
                + repr('import os,time;from pathlib import Path;print("partial before signal",flush=True);Path(' + repr(str(base / 'ready')) + ').write_text(str(os.getpid()));time.sleep(30)')
                + '],timeout=30)\nexcept KeyboardInterrupt:\n assert not processes.ACTIVE\n raise SystemExit(23)\n')
            process = subprocess.Popen([sys.executable, str(script)], stdout=subprocess.PIPE, stderr=subprocess.PIPE, start_new_session=True)
            try:
                deadline = time.monotonic() + 5
                while not (base / 'ready').exists() and time.monotonic() < deadline:
                    self.assertIsNone(process.poll(), 'signal control exited before ready')
                    time.sleep(0.01)
                self.assertTrue((base / 'ready').exists())
                os.kill(process.pid, signal.SIGTERM)
                _, err = process.communicate(timeout=5)
                self.assertEqual(process.returncode, 23, err.decode())
                owner = object.__new__(Owner)
                owner.output = base / 'output'
                row = self.evidence(owner)
                self.assertTrue(row['interrupted'])
                self.assertFalse(row['deadline'])
                self.assertEqual(row['harness_failure']['type'], 'StageInterrupted')
                self.assertEqual((owner.output / row['stdout']).read_bytes(), b'partial before signal\n')
                self.assertIn(state(int((base / 'ready').read_text())), {None, 'Z', 'X'})
            finally:
                if process.poll() is None:
                    os.killpg(process.pid, signal.SIGKILL)
                process.communicate(timeout=3)

    def test_creation_interrupt_recovers_new_handle_and_preserves_other_actor(self):
        with tempfile.TemporaryDirectory() as temporary:
            owner = Owner(Path(temporary) / 'output')
            sentinel = processes.start([sys.executable, '-c', 'import time;time.sleep(30)'],
                stdout=subprocess.PIPE, stderr=subprocess.PIPE)
            previous = {sig: signal.getsignal(sig) for sig in [signal.SIGINT, signal.SIGTERM]}
            real_popen = subprocess.Popen
            def start_then_interrupt(*args, **kwargs):
                process = real_popen(*args, **kwargs)
                if not select.select([process.stdout], [], [], 5)[0]:
                    os.killpg(process.pid, signal.SIGKILL)
                    process.communicate(timeout=2)
                    raise AssertionError('creation control child did not write ready bytes')
                # start() is still deferring signals and has not assigned its
                # return value or added this child to its ownership registry.
                os.kill(os.getpid(), signal.SIGTERM)
                return process
            try:
                processes.install_handlers()
                with mock.patch.object(processes.subprocess, 'Popen', side_effect=start_then_interrupt):
                    with self.assertRaises(processes.StageInterrupted):
                        commands.run(owner, 'creation-interrupt', [sys.executable, '-c',
                            'import os,time;os.write(1,b"created before assignment\\n");time.sleep(30)'])
                row = self.evidence(owner)
                self.assertIsNotNone(row['pid'])
                self.assertTrue(row['interrupted'])
                self.assertFalse(row['deadline'])
                self.assertEqual((owner.output / row['stdout']).read_bytes(), b'created before assignment\n')
                self.assertIn(state(row['pid']), {None, 'Z', 'X'})
                self.assertIsNone(sentinel.poll(), 'another owned actor was stopped')
                self.assertEqual(set(processes.ACTIVE), {sentinel.pid})
            finally:
                for sig, handler in previous.items():
                    signal.signal(sig, handler)
                processes.stop(sentinel)
                processes.forget(sentinel)


if __name__ == '__main__':
    unittest.main()
