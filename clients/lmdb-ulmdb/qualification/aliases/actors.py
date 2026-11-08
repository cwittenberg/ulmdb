"""Owned, bounded interactive LMDB actors; no observer changes transaction work."""
import os
from pathlib import Path
import queue
import re
import signal
import subprocess
import threading
import time
import processes


class Agent:
    def __init__(self, label, database, flags, case, executable, env):
        self.label = label
        self.row = {'label': label, 'argv': [str(executable), 'agent', str(database), str(flags)], 'flags': flags, 'events': []}
        case['agents'].append(self.row)
        folder = Path(case['directory'])
        self.out = folder / (label + '.stdout')
        self.err = folder / (label + '.stderr')
        self.output = self.out.open('w')
        self.errors = self.err.open('w')
        self.q = queue.Queue()
        self.ready = False
        self.expected_exit = 0
        self.p = processes.start(self.row['argv'], env=env, stdin=subprocess.PIPE, stdout=subprocess.PIPE, stderr=self.errors, text=True)
        def pump():
            for line in self.p.stdout:
                self.output.write(line)
                self.output.flush()
                self.q.put(line.rstrip())
            self.q.put(None)
        self.thread = threading.Thread(target=pump)
        self.thread.start()
        self.expect('OPENING')

    def expect(self, prefix, timeout=6):
        end = time.monotonic() + timeout
        while True:
            line = self.q.get(timeout=max(.001, end - time.monotonic()))
            if line is None:
                raise RuntimeError(self.label + ' EOF before ' + prefix + ': ' + self.err.read_text())
            if line.startswith(prefix):
                return line

    def opened(self, allow_error=False):
        line = self.expect('OPEN_RESULT')
        rc = int(line.split('rc=')[1])
        self.row['open_result'] = rc
        if rc:
            if not allow_error:
                raise RuntimeError(self.label + ' open failed: ' + str(rc))
            self.p.wait(timeout=6)
            return False
        self.expect('READY')
        self.ready = True
        return True

    def send(self, text):
        self.row['events'].append({'send': text})
        self.p.stdin.write(text + '\n')
        self.p.stdin.flush()
        self.expect('DONE ' + text.split()[0])

    def send_copy_unsupported(self, destination):
        text = 'copy ' + str(destination) + ' 0'
        self.row['events'].append({'send': text, 'expected_error': 95})
        self.expected_exit = 2  # The unchanged C actor's checked call exits2.
        self.p.stdin.write(text + '\n')
        self.p.stdin.flush()
        self.p.wait(timeout=10)
        self.finish()
        if 'COPIED' in self.out.read_text() or not re.search(r'ERROR where=copy rc=95(?:\s|$)', self.err.read_text()):
            raise RuntimeError('copy did not return the explicit ENOTSUP boundary')

    def assert_view(self, label, sequence, value, first=None, second=None):
        text = self.out.read_text()
        ids = re.findall(r'^TXN label=' + re.escape(label) + r' id=(\d+)$', text, re.MULTILINE)
        if ids != [str(sequence)]:
            raise RuntimeError(self.label + ': wrong read ID at ' + label + ': ' + str(ids))
        expected = {'k': value, 'first': first, 'second': second}
        for key, wanted in expected.items():
            values = re.findall(r'^VALUE label=' + re.escape(label) + r' dbi=1 key=' + re.escape(key) + r' rc=(-?\d+) bytes=([0-9a-f]*)$', text, re.MULTILINE)
            correct = [('0', wanted.encode().hex())] if wanted is not None else [('-30798', '')]
            if values != correct:
                raise RuntimeError(self.label + ': wrong get rc/value at ' + label + '/' + key + ': ' + str(values))
        self.row.setdefault('verified_views', []).append({'label': label, 'id': sequence, 'values': expected})

    def assert_abort(self, writer_id, value, first=None, second=None):
        self.assert_view('writer-base', writer_id, value, first, second)
        text = self.out.read_text()
        writers = re.findall(r'^WRITER id=(\d+) action=abort$', text, re.MULTILINE)
        if writers != [str(writer_id)] or text.splitlines().count('ABORTED') != 1:
            raise RuntimeError(self.label + ': missing/wrong aborted writer ID')
        self.row['verified_aborted_writer_id'] = writer_id

    def blocked(self, label):
        try:
            line = self.q.get(timeout=.2)
            raise RuntimeError(self.label + ' unexpectedly progressed at ' + label + ': ' + str(line))
        except queue.Empty:
            pass
        rows = [line for line in Path('/proc/locks').read_text().splitlines()
                if '->' in line and str(self.p.pid) in line.split()]
        wchan = Path('/proc/' + str(self.p.pid) + '/wchan').read_text().strip()
        if not rows and not any(word in wchan for word in ['lock', 'futex']):
            raise RuntimeError('no kernel waiter evidence for ' + self.label + ': ' + wchan)
        self.row['events'].append({'phase': label, 'kernel_waiter_rows': rows, 'wchan': wchan})

    def kill(self):
        os.killpg(self.p.pid, signal.SIGKILL)
        self.p.wait(timeout=6)
        self.expected_exit = -signal.SIGKILL
        self.row['intentional_sigkill'] = True
        self.finish()

    def close(self):
        if self.output.closed:
            return
        if self.p.poll() is None and self.ready:
            self.p.stdin.write('close\n')
            self.p.stdin.flush()
            self.expect('CLOSED')
            self.p.wait(timeout=6)
        self.finish()

    def finish(self):
        if self.output.closed:
            return
        if self.p.poll() is None:
            # Keep the one stdout owner (pump) throughout termination. Generic
            # registry cleanup is a fallback for interruption during creation.
            try:
                os.killpg(self.p.pid, signal.SIGTERM)
            except ProcessLookupError:
                pass
            try:
                self.p.wait(timeout=1)
            except subprocess.TimeoutExpired:
                os.killpg(self.p.pid, signal.SIGKILL)
                self.p.wait(timeout=2)
        self.thread.join(timeout=2)
        if self.thread.is_alive():
            raise RuntimeError('owned actor pump did not terminate')
        self.output.close()
        self.errors.close()
        processes.forget(self.p)
        self.row.update(exit_code=self.p.returncode, stdout=str(self.out), stderr=str(self.err))
        if self.p.returncode != self.expected_exit:
            raise RuntimeError(self.label + ' exit ' + str(self.p.returncode))
