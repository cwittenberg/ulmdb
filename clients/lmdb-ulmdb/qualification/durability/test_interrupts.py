#!/usr/bin/env python3
"""Real SIGINT/SIGTERM harness controls, never LMDB provider evidence."""
from pathlib import Path
from contextlib import contextmanager
import argparse
import json
import os
import signal
import subprocess
import sys
import tempfile
import time
import unittest

HERE = Path(__file__).resolve().parent
WORKER = r'''import json,os,signal,time
from pathlib import Path
signal.signal(signal.SIGTERM,signal.SIG_IGN)
ready=Path(os.environ['DURABILITY_HARNESS_READY'])
child=os.fork()
if child==0:
 signal.signal(signal.SIGTERM,signal.SIG_IGN)
 ready.with_suffix('.child').write_text(str(os.getpid()))
 while True:time.sleep(1)
while not ready.with_suffix('.child').exists():time.sleep(.01)
os.write(1,b'PARTIAL')
os.write(2,b'partial-stderr\n')
ready.write_text(json.dumps({'parent':os.getpid(),'child':child}))
if os.environ.get('DURABILITY_HARNESS_EXIT_PARENT'):os._exit(0)
while True:time.sleep(1)
'''

def state(pid):
    try:
        return Path(f'/proc/{pid}/stat').read_text().split()[2]
    except FileNotFoundError:
        return None


@contextmanager
def case_directory(role, signum):
    parent = os.environ.get('ULMDB_DURABILITY_INTERRUPT_ARTIFACTS')
    if parent:
        path = Path(parent) / f'{role}-{signum}'
        path.mkdir(parents=True, exist_ok=False)
        yield str(path)
    else:
        with tempfile.TemporaryDirectory() as temporary:
            yield temporary


class InterruptedTools(unittest.TestCase):
    def one(self, role, signum):
        with case_directory(role, signum) as temporary:
            base = Path(temporary)
            ready = base / 'ready.json'
            worker = base / 'worker'
            worker.write_text('#!' + sys.executable + '\n' + WORKER)
            worker.chmod(0o755)
            env = dict(os.environ, DURABILITY_HARNESS_READY=str(ready))
            output = base / 'output'
            if role == 'helper':
                source = base / 'native'
                source.mkdir()
                for name in ['Cargo.toml', 'Cargo.lock']:
                    (source / name).write_text('')
                (source / 'rust-toolchain.toml').write_text('[toolchain]\nchannel="unused-test-toolchain"\n')
                for name in ['ulmdb', 'ulmdb-core', 'ulmdb-storage', 'ulmdb-kv', 'ulmdb-prefix']:
                    (source / 'crates' / name).mkdir(parents=True)
                    (source / 'crates' / name / 'Cargo.toml').write_text('')
                bindir = base / 'bin'
                bindir.mkdir()
                (bindir / 'cargo').symlink_to(worker)
                env['PATH'] = str(bindir)  # No rustup invocation or installation.
                if signum == signal.SIGTERM:
                    env['DURABILITY_HARNESS_EXIT_PARENT'] = '1'
                argv = [sys.executable, str(HERE / 'build_backend_init.py'), '--source-root', str(source), '--output', str(output)]
                receipt = output / 'provenance.json'
            elif role == 'runner':
                env['CC'] = str(worker)
                argv = [sys.executable, str(HERE / 'run.py'), '--source-root', str(base), '--package', str(base / 'unused'),
                        '--archive', str(base / 'unused.a'), '--lmdb-source', str(base), '--legacy-source', str(base),
                        '--current09-source', str(base), '--backend-init', str(base / 'unused-init'),
                        '--backend-init-provenance', str(base / 'unused.json'), '--output', str(output)]
                receipt = output / 'receipt.json'
            else:
                # Synthetic setup isolates the real interactive owned-process
                # path. No fake LMDB provider is accepted or qualified.
                wrapper = base / 'interactive.py'
                wrapper.write_text('''from pathlib import Path
import hashlib,json,subprocess,sys,types
p=Path(sys.argv[1]);m=types.ModuleType('core');m.__file__=str(p)
exec(compile(p.read_bytes(),str(p),'exec'),m.__dict__)
m.OWNED.install_handlers()
out=Path(sys.argv[3]);q=m.Qualification(types.SimpleNamespace(output=out,timeout=600))
app=Path(sys.argv[2]);provider=out/'provider';provider.write_bytes(b'')
q.helper=app;q.check_mode=lambda *unused:'ULMDBCUR';q.successful=lambda *unused:b''
q.command=lambda *unused,**kwargs:subprocess.CompletedProcess([],0,str(provider).encode(),b'')
q.apps['copies']={'path':str(app),'sha256':hashlib.sha256(app.read_bytes()).hexdigest(),'imports':[]}
try:q.weak_copy('legacy',provider)
except BaseException as error:
 with m.OWNED.ignore_interrupts():
  m.OWNED.cleanup_all(out);q.receipt.update(complete=False,passed=False,error=str(error));q.save()
 sys.exit(1)
''')
                argv = [sys.executable, str(wrapper), str(HERE / 'run.py'), str(worker), str(output)]
                receipt = output / 'receipt.json'
            tool = subprocess.Popen(argv, env=env, stdout=subprocess.PIPE, stderr=subprocess.PIPE, start_new_session=True)
            owned = None
            try:
                deadline = time.monotonic() + 15
                while time.monotonic() < deadline:
                    try:
                        owned = json.loads(ready.read_text())
                        break
                    except (FileNotFoundError, json.JSONDecodeError):
                        if tool.poll() is not None:
                            out, err = tool.communicate()
                            self.fail(f'{role} exited before live child: {out!r} {err!r}')
                        time.sleep(.01)
                self.assertIsNotNone(owned, 'confirmed-live worker did not start')
                self.assertNotIn(state(owned['child']), {None, 'Z'}, 'descendant was not alive before interruption')
                os.kill(tool.pid, signum)
                out, err = tool.communicate(timeout=8)
                self.assertNotEqual(tool.returncode, 0)
                self.assertTrue(receipt.is_file(), (out, err))
                data = json.loads(receipt.read_text())
                self.assertFalse(data['complete'])
                self.assertFalse(data['passed'])
                for pid in owned.values():
                    deadline = time.monotonic() + 3
                    while state(pid) not in {None, 'Z'} and time.monotonic() < deadline:
                        time.sleep(.01)
                    self.assertIn(state(pid), {None, 'Z'}, f'owned process {pid} survived {role} signal{signum}')
                retained_out = b''.join(p.read_bytes() for p in output.rglob('*') if p.is_file() and (p.name == 'stdout' or p.suffix == '.stdout'))
                retained_err = b''.join(p.read_bytes() for p in output.rglob('*') if p.is_file() and (p.name == 'stderr' or p.suffix == '.stderr'))
                self.assertIn(b'PARTIAL', retained_out)
                self.assertIn(b'partial-stderr', retained_err)
            finally:
                if tool.poll() is None:
                    os.killpg(tool.pid, signal.SIGKILL)
                    tool.communicate(timeout=3)
                if owned:
                    for pid in owned.values():
                        if state(pid) not in {None, 'Z'}:
                            try:os.kill(pid,signal.SIGKILL)
                            except ProcessLookupError:pass

    def test_helper_sigint(self): self.one('helper', signal.SIGINT)
    def test_helper_sigterm_with_exited_leader_and_live_descendant(self): self.one('helper', signal.SIGTERM)
    def test_runner_sigint(self): self.one('runner', signal.SIGINT)
    def test_runner_sigterm(self): self.one('runner', signal.SIGTERM)
    def test_interactive_sigint_partial_line(self): self.one('interactive', signal.SIGINT)
    def test_interactive_sigterm_partial_line(self): self.one('interactive', signal.SIGTERM)


if __name__ == '__main__':
    parser = argparse.ArgumentParser(add_help=False)
    parser.add_argument('--artifacts', type=Path)
    options, remaining = parser.parse_known_args()
    if options.artifacts:
        options.artifacts.resolve().mkdir(parents=True, exist_ok=False)
        os.environ['ULMDB_DURABILITY_INTERRUPT_ARTIFACTS'] = str(options.artifacts.resolve())
    unittest.main(argv=[sys.argv[0]] + remaining)
