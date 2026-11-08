#!/usr/bin/env python3
"""Persistent real-process controls for the enclosing qualification owner."""
from pathlib import Path
from contextlib import contextmanager
import argparse
import hashlib
import json
import os
import signal
import subprocess
import sys
import tempfile
import time
import types
import unittest

HERE = Path(__file__).resolve().parent
QUALIFIER = HERE.parent / 'qualify.py'
ARTIFACTS = None
WORKER = '''from pathlib import Path
import json,os,signal,sys,time
signal.signal(signal.SIGTERM,signal.SIG_IGN)
ready=Path(sys.argv[1]);child=os.fork()
if child==0:
 signal.signal(signal.SIGTERM,signal.SIG_IGN)
 ready.with_suffix('.child').write_text(str(os.getpid()))
 print('partial-child',flush=True)
 while True:time.sleep(1)
while not ready.with_suffix('.child').exists():time.sleep(.01)
print('partial-leader',flush=True)
ready.write_text(json.dumps({'leader':os.getpid(),'child':child}))
if sys.argv[2]=='fail':os._exit(7)
while True:time.sleep(1)
'''
WRAPPER = '''from pathlib import Path
import sys,types
p=Path(sys.argv[1]);m=types.ModuleType('qualified_parent');m.__file__=str(p)
exec(compile(p.read_bytes(),str(p),'exec'),m.__dict__)
m.install_interrupt_handlers()
q=m.Qualification(Path(sys.argv[2]),600,False)
try:q.run('owned-stage',[sys.executable,sys.argv[3],sys.argv[4],sys.argv[5]])
except BaseException as error:
 with m.ignore_interrupts():
  m.stop_stages();q.receipt.update(complete=False,passed=False,error=str(error));q.save()
 sys.exit(1)
'''

def state(pid):
    try:return Path(f'/proc/{pid}/stat').read_text().rsplit(')',1)[1].split()[0]
    except FileNotFoundError:return None

@contextmanager
def directory(label):
    if ARTIFACTS:
        base = ARTIFACTS / label
        base.mkdir(parents=True, exist_ok=False)
        yield base
    else:
        with tempfile.TemporaryDirectory() as temporary:
            yield Path(temporary)


class OuterGateOwnership(unittest.TestCase):
    def run_case(self, label, signum=None):
        with directory(label) as base:
            worker, wrapper = base/'worker.py', base/'wrapper.py'
            worker.write_text(WORKER);wrapper.write_text(WRAPPER)
            ready = base/'ready.json'
            unrelated = subprocess.Popen([sys.executable,'-c','import time;time.sleep(60)'],start_new_session=True)
            command = [sys.executable,str(wrapper),str(QUALIFIER),str(base/'outer'),str(worker),str(ready),'fail' if signum is None else 'live']
            owner = subprocess.Popen(command,stdout=subprocess.PIPE,stderr=subprocess.PIPE,start_new_session=True)
            pids = None
            record = {'qualifier':str(QUALIFIER),'qualifier_sha256':hashlib.sha256(QUALIFIER.read_bytes()).hexdigest(),
                      'command':command,'signal':signum,'unrelated_pid':unrelated.pid,'passed':False}
            try:
                deadline = time.monotonic()+15
                while time.monotonic()<deadline:
                    try:pids=json.loads(ready.read_text());break
                    except (FileNotFoundError,json.JSONDecodeError):
                        if owner.poll()is not None:
                            out,err=owner.communicate();self.fail(f'qualifier ended before live stage {out!r} {err!r}')
                        time.sleep(.01)
                self.assertIsNotNone(pids)
                self.assertNotIn(state(pids['child']),{None,'Z'})
                record['confirmed_owned_pids'] = pids
                if signum is not None:
                    self.assertNotIn(state(pids['leader']),{None,'Z'})
                    os.kill(owner.pid,signum)
                out,err=owner.communicate(timeout=15)
                (base/'owner.stdout').write_bytes(out);(base/'owner.stderr').write_bytes(err)
                record['returncode']=owner.returncode
                self.assertNotEqual(owner.returncode,0)
                final={role:state(pid)for role,pid in pids.items()}
                record['owned_after_return']=final
                self.assertTrue(all(value in{None,'Z'}for value in final.values()),'failed/exited leader lost its live descendant')
                self.assertIsNone(unrelated.poll(),'cleanup signaled an unrelated session')
                receipt=json.loads((base/'outer/receipt.json').read_text())
                self.assertFalse(receipt['complete']);self.assertFalse(receipt['passed'])
                step=receipt['steps'][0]
                cleanup=step['owned_cleanup'][0]
                self.assertEqual(cleanup['identity']['pid'],pids['leader'])
                self.assertEqual(cleanup['identity']['pid'],cleanup['identity']['pgid'])
                self.assertEqual(cleanup['identity']['pid'],cleanup['identity']['sid'])
                self.assertGreater(cleanup['identity']['starttime'],0)
                self.assertEqual(cleanup['after'],[])
                self.assertTrue(cleanup.get('escalated_to_kill'))
                log=Path(step['log']).read_text()
                self.assertIn('partial-child',log);self.assertIn('partial-leader',log)
                record.update(passed=True,receipt=receipt,unrelated_session_survived=True)
            except BaseException as error:
                record['error']={'type':type(error).__name__,'message':str(error)}
                if pids:record['owned_at_failure']={role:state(pid)for role,pid in pids.items()}
                raise
            finally:
                if owner.poll()is None:
                    os.killpg(owner.pid,signal.SIGKILL);owner.communicate(timeout=3)
                record['operator_cleanup_pids']=[]
                if pids:
                    for pid in pids.values():
                        if state(pid)not in{None,'Z'}:
                            record['operator_cleanup_pids'].append(pid)
                            try:os.kill(pid,signal.SIGKILL)
                            except ProcessLookupError:pass
                if unrelated.poll()is None:
                    os.killpg(unrelated.pid,signal.SIGKILL);unrelated.wait(timeout=3)
                (base/'control.json').write_text(json.dumps(record,indent=2)+'\n')

    def test_failed_leader_keeps_descendant_until_owned_cleanup(self):self.run_case('failed-leader')
    def test_sigint_owned_group_cleanup(self):self.run_case('sigint',signal.SIGINT)
    def test_sigterm_owned_group_cleanup(self):self.run_case('sigterm',signal.SIGTERM)


class SourceFingerprintInputs(unittest.TestCase):
    def test_compiled_clients_docker_policy_and_embedded_assets_detect_mutation(self):
        module = types.ModuleType('source_fingerprint_control')
        module.__file__ = str(QUALIFIER)
        exec(compile(QUALIFIER.read_bytes(), str(QUALIFIER), 'exec'), module.__dict__)
        with tempfile.TemporaryDirectory(prefix='ulmdb-source-input-control-') as temporary:
            root = Path(temporary)
            subprocess.run(['git', 'init', '-q', str(root)], check=True)
            inputs = {
                'clients/cpp/tests/client.cpp': b'int main() {}',
                'clients/cpp/include/ulmdb/types.hpp': b'#pragma once',
                'clients/node/index.js': b'module.exports = {};',
                'clients/node/scripts/build.js': b'// native build',
                'clients/node/index.d.ts': b'export {};',
                'clients/lmdb-ulmdb/exports.map': b'{ global: mdb_*; };',
                'clients/php/Dockerfile.dockerignore': b'**\n!Cargo.toml\n',
                '.dockerignore': b'**\n',
                'utility/explorer/src/main.rs': b'const B: &[u8] = include_bytes!("banner.bin");',
                'utility/explorer/src/banner.bin': b'embedded build input',
                'support/lmdb/source.toml': b'schema = 1\n',
            }
            excluded = {
                'support/lmdb/vendor/mdb.c': b'pinned separately',
                'clients/lmdb-ulmdb/conformance/receipt.json': b'{}',
                'clients/node/build/ulmdb.node': b'generated native binary',
            }
            for relative, value in {**inputs, **excluded}.items():
                path = root / relative
                path.parent.mkdir(parents=True, exist_ok=True)
                path.write_bytes(value)
            module.ROOT = root
            before = module.source_fingerprint()
            self.assertEqual(set(before), set(inputs))
            for relative, value in inputs.items():
                with self.subTest(path=relative):
                    path = root / relative
                    path.write_bytes(value + b'\nchanged')
                    after = module.source_fingerprint()
                    self.assertEqual(set(after), set(before))
                    self.assertEqual([p for p in before if before[p] != after[p]], [relative])
                    path.write_bytes(value)
            for relative in excluded:
                (root / relative).write_bytes(b'output changed')
            self.assertEqual(module.source_fingerprint(), before)


if __name__=='__main__':
    parser=argparse.ArgumentParser(add_help=False)
    parser.add_argument('--artifacts',type=Path)
    parser.add_argument('--qualifier',type=Path)
    options,remaining=parser.parse_known_args()
    if options.artifacts:
        options.artifacts.resolve().mkdir(parents=True,exist_ok=False);ARTIFACTS=options.artifacts.resolve()
    if options.qualifier:QUALIFIER=options.qualifier.resolve()
    unittest.main(argv=[sys.argv[0]]+remaining)
