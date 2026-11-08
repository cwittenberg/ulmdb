#!/usr/bin/env python3
"""Linux NORDAHEAD public contract, actual VMA advice and deficient-advice control.

Compile each executable only against a pinned reference SONAME, then swap actual
providers without recompiling. No performance or power-loss claim is made.
"""
from __future__ import annotations
import argparse, hashlib, json, os, re, shutil, subprocess, sys, types
from datetime import datetime, timezone
from pathlib import Path
ROOT = Path(__file__).resolve().parents[3]
HERE = Path(__file__).resolve().parent
HELPER_SHA = 'd3122fa934234dfff338080293fb81d4aa837704e28b108c9f8f41238a45c7e2'
PROCESSES_SHA = 'a34d2c39bb45b326acaa4a47f8be7a823af5970b4bd647f59bc9436697239be3'

def digest(path):
    h = hashlib.sha256()
    with Path(path).open('rb') as stream:
        while block := stream.read(1024 * 1024): h.update(block)
    return h.hexdigest()

def require(yes, message):
    if not yes: raise RuntimeError(message)

class Run:
    def __init__(self, args):
        self.a = args
        self.out = args.output.resolve()
        self.out.mkdir(parents=True, exist_ok=False)
        self.inputs = {}
        self.apps = {}
        self.commands = []
        self.cases = []
        self.data = {'schema': 1, 'profile': 'linux-file-read-ahead-v1', 'complete': False,
            'passed': False, 'product_qualified': False, 'scope': 'Linux per-VMA advisory request; no performance, power-loss or other-platform qualification',
            'commands': self.commands, 'cases': self.cases, 'inputs': self.inputs, 'executables': self.apps}
        helper = ROOT / 'clients/lmdb-ulmdb/qualification/durability/processes.py'
        helper_bytes = helper.read_bytes()
        require(hashlib.sha256(helper_bytes).hexdigest() == PROCESSES_SHA, 'owned process helper changed')
        self.owned = types.ModuleType('read_ahead_owned_processes')
        self.owned.__file__ = str(helper)
        exec(compile(helper_bytes, str(helper), 'exec'), self.owned.__dict__)
        self.owned.install_handlers()
        require(self.protect(helper) == PROCESSES_SHA, 'owned helper changed after compilation')
        self.env = {k:v for k,v in os.environ.items() if not k.startswith(('LD_', 'ULMDB_'))}
        self.env['LD_BIND_NOW'] = '1'
        self.save()

    def save(self):
        (self.out / 'receipt.json').write_text(json.dumps(self.data, indent=2) + '\n')

    def protect(self, path):
        path = Path(path).absolute()
        require(path.is_file() and not path.is_symlink(), 'expected regular input: ' + str(path))
        path = path.resolve()
        value = digest(path)
        require(str(path) not in self.inputs or self.inputs[str(path)] == value, 'input changed')
        self.inputs[str(path)] = value
        return value

    def command(self, label, argv, env=None, expected=0):
        folder = self.out / 'commands' / label
        folder.mkdir(parents=True, exist_ok=False)
        row = {'label': label, 'command': [str(x) for x in argv],
               'expected_exit': expected, 'state': 'starting', 'creator_pid': os.getpid()}
        self.commands.append(row)
        self.save()
        process = None
        try:
            process = self.owned.start(row['command'], env=env or self.env,
                                       stdout=subprocess.PIPE, stderr=subprocess.PIPE)
            stat = Path('/proc', str(process.pid), 'stat').read_text().rsplit(')', 1)[1].split()
            row.update(pid=process.pid, pgid=os.getpgid(process.pid), sid=os.getsid(process.pid),
                       starttime=int(stat[19]), state='running')
            require(row['pgid'] == row['sid'] == row['pid'], 'child group ownership mismatch')
            self.save()
            stdout, stderr = process.communicate(timeout=self.a.timeout)
        except BaseException as exc:
            if process is not None:
                with self.owned.ignore_interrupts(): stdout, stderr = self.owned.stop(process)
                (folder/'stdout').write_bytes(stdout or b'')
                (folder/'stderr').write_bytes(stderr or b'')
                row['exit_code'] = process.returncode
                self.owned.forget(process)
            else:
                row['registered_during_creation'] = list(self.owned.ACTIVE)
            row.update(state='interrupted' if isinstance(exc, KeyboardInterrupt) else 'failed',
                       error=type(exc).__name__ + ': ' + str(exc))
            self.save()
            raise
        self.owned.forget(process)
        (folder/'stdout').write_bytes(stdout)
        (folder/'stderr').write_bytes(stderr)
        row.update(exit_code=process.returncode, stdout_sha256=digest(folder/'stdout'),
                   stderr_sha256=digest(folder/'stderr'),
                   state='finished' if process.returncode == expected else 'unexpected-exit')
        self.save()
        require(process.returncode == expected, f'{label}: exit {process.returncode}: {stderr[-1200:]!r}')
        return stdout, stderr

    def execute(self, label, app, library, abi, *, mode='kernel', preload=None, expected=0, backend=None):
        folder = self.out / 'runs' / label
        folder.mkdir(parents=True, exist_ok=False)
        database = folder / 'database'
        if backend:
            self.command(label+'-init', [self.backend_init, backend, database])
            current = (database/'CURRENT').read_bytes()
            pointer = current if len(current)==64 else current[16:80]
            require(pointer[:8] == {'legacy':b'ULMDBCUR','cohort':b'ULMDBC03','indexed':b'ULMDBT04'}[backend], 'wrong actual initialized format')
        env = dict(self.env, LD_LIBRARY_PATH=str(library.parent), ULMDB_LMDB_ABI=abi)
        if preload: env['LD_PRELOAD'] = str(preload)
        loader, _ = self.command(label+'-loader', [app], dict(env, LD_TRACE_LOADED_OBJECTS='1'))
        require(str(library).encode() in loader, 'wrong loaded provider')
        trace = folder/'syscalls.trace'
        env['LD_DEBUG'] = 'libs,bindings'
        output, error = self.command(label, ['strace','-f','-qq','-yy','-s','4096','-e','trace=mmap,madvise,munmap', '-o',trace, app,database,mode], env, expected)
        require(f'PROVIDER {library}'.encode() in error, 'public C provider witness mismatch')
        for name in self.apps[str(app)]['imports']:
            require(re.search(rb'binding file '+re.escape(str(app).encode())+rb'.* to '+re.escape(str(library).encode())+rb'.*symbol [\x27`]'+name.encode()+rb"['`]", error), 'actual import selected another provider: '+name)
        calls = [line for line in trace.read_text().splitlines() if 'MADV_RANDOM' in line]
        if expected == 0:
            require(output.endswith(b'noreadahead-contract=ok\n'), 'data/ID/reopen oracle did not finish')
            if mode == 'kernel': require(calls and all(line.rstrip().endswith('= 0') for line in calls), 'no successful real random advice')
        else:
            require(b'oracle: real kernel random advice\n' in error and b'call:' not in error, 'deficient advice rejected for an unrelated reason')
            require(b'phase=0 nordahead=0 first=1 second=2' in output and b'noreadahead-contract=ok' not in output, 'negative failed before ordinary control')
            if preload: require(not calls, 'no-op advice interposer did not remove actual random syscalls')
        self.cases.append({'label':label,'backend':backend or 'legacy','mode':mode,'expected_exit':expected,'executable_sha256':digest(app),'provider_sha256':digest(library),'trace_sha256':digest(trace),'random_advice_calls':len(calls),'passed':True})
        self.save()
        return output

    def run(self):
        require(sys.platform == 'linux', 'kernel advice qualification requires Linux')
        source = self.out/'read_ahead.c'
        origin_hash = self.protect(HERE/'read_ahead.c')
        shutil.copy2(HERE/'read_ahead.c',source)
        require(self.protect(source) == origin_hash, 'copied C fixture differs from protected origin')
        self.protect(Path(__file__))
        original = (self.a.package/'lib/liblmdb.so.0' if self.a.package.is_dir() else self.a.package).resolve()
        package = self.out/'provider/liblmdb.so.0'; package.parent.mkdir(); shutil.copy2(original,package)
        self.data['provider'] = {'original':str(original),'sha256':self.protect(original)}
        self.protect(package)
        self.backend_init = None
        if self.a.backend_init:
            provenance = json.loads(self.a.backend_init_provenance.read_text())
            require(provenance.get('complete') and provenance.get('passed') and provenance.get('source_before_after_equal') and provenance.get('role')=='empty-native-backend-initializer', 'invalid empty initializer provenance')
            require(provenance['entry_source_sha256']==HELPER_SHA and digest(self.a.backend_init)==provenance['executable_sha256'], 'wrong empty initializer')
            require(all(c.get('returncode')==0 for c in provenance['commands']), 'initializer build failed')
            for item in provenance['sources']:
                p=Path(item['path']); p=p if p.is_absolute() else self.a.backend_init_provenance.parent/p
                require(self.protect(p)==item['sha256'], 'initializer source drift')
            self.protect(self.a.backend_init_provenance); self.protect(self.a.backend_init)
            self.backend_init=self.out/'empty-backend-init';shutil.copy2(self.a.backend_init,self.backend_init)
            self.data['initializer_scope']='Only seq0 physical layout; no C/advice qualification inherited'
        expected = None
        modern_app = None
        for version,pin,origin in [('0.9.31','legacy',self.a.legacy_source),('0.9.36','current09',self.a.current09_source),('1.0.2','modern',self.a.lmdb_source)]:
            if (origin/'libraries/liblmdb').is_dir(): origin=origin/'libraries/liblmdb'
            manifest=ROOT/f'clients/lmdb-ulmdb/{pin}-source.json'; metadata=json.loads(manifest.read_text());self.protect(manifest)
            folder=self.out/'references'/version; copied=folder/'source';copied.mkdir(parents=True)
            for item in metadata['files']:
                p=origin/Path(item['path']).name;require(self.protect(p)==item['sha256'] and p.stat().st_size==item['bytes'], 'reference pin mismatch');shutil.copy2(p,copied/p.name);self.protect(copied/p.name)
            reference=folder/'liblmdb.so.0'
            self.command('build-'+version,['cc','-shared','-fPIC','-O2','-pthread','-D_GNU_SOURCE',copied/'mdb.c',copied/'midl.c','-Wl,-soname,liblmdb.so.0','-o',reference]);(folder/'liblmdb.so').symlink_to('liblmdb.so.0')
            app=folder/'probe';self.command('compile-'+version,['cc','-std=c11','-O2','-Wall','-Wextra','-Werror','-I',copied,source,'-L',folder,'-llmdb','-ldl','-o',app]);self.protect(app);self.protect(reference)
            dynamic,_=self.command('dynamic-'+version,['readelf','-d',app]);require(b'[liblmdb.so.0]' in dynamic and b'RPATH' not in dynamic and b'RUNPATH' not in dynamic,'probe hardcodes a provider')
            imports,_=self.command('imports-'+version,['nm','-D','--undefined-only',app]);names=sorted(set(re.findall(rb'\b(mdb_\w+)\b',imports)));require(names,'no public imports');self.apps[str(app)]={'sha256':digest(app),'imports':[n.decode() for n in names]}
            abi='1.0' if version=='1.0.2' else version
            reference_output=self.execute('reference-'+version,app,reference,abi)
            require(expected is None or expected==reference_output,'pinned logical traces disagree');expected=reference_output
            actual=self.execute('candidate-'+version,app,package,abi);require(actual==expected,'candidate logical trace differs')
            if version=='1.0.2':modern_app=app
        noop=self.out/'noop.c';noop.write_text('#define _GNU_SOURCE\n#include <dlfcn.h>\n#include <sys/mman.h>\nint madvise(void*p,size_t n,int a){if(a==MADV_RANDOM)return 0;int(*real)(void*,size_t,int)=dlsym(RTLD_NEXT,"madvise");return real(p,n,a);}\n')
        interposer=self.out/'noop.so';self.command('compile-noop',['cc','-shared','-fPIC','-O2',noop,'-ldl','-o',interposer]);self.protect(noop);self.protect(interposer)
        self.execute('noop-kernel-rejection',modern_app,package,'1.0',preload=interposer,expected=2)
        require(self.execute('noop-unrelated-logical',modern_app,package,'1.0',preload=interposer,mode='logical')==expected,'no-op changed unrelated public calls')
        if self.backend_init:
            for backend in ['legacy','cohort','indexed']:
                require(self.execute('backend-'+backend,modern_app,package,'1.0',backend=backend)==expected,'backend logical trace differs')
        for p,h in self.inputs.items():require(digest(p)==h,'input changed during run: '+p)
        self.data.update(complete=True,passed=True,reference_versions=3,required_noop_rejections=1,source_before_after_equal=True)
        self.save()

def main():
    p=argparse.ArgumentParser(description=__doc__)
    p.add_argument('--package',type=Path,required=True,help='actual packaged directory or actual shared library')
    p.add_argument('--output',type=Path,default=ROOT/'target/read-ahead-qualification'/f'{datetime.now(timezone.utc):%Y%m%dT%H%M%SZ}-{os.getpid()}')
    p.add_argument('--legacy-source',type=Path,default=Path.home()/'.cache/ulmdb-compat/lmdb-0.9.31')
    p.add_argument('--current09-source',type=Path,default=Path.home()/'.cache/ulmdb-compat/lmdb-0.9.36')
    p.add_argument('--lmdb-source',type=Path,default=ROOT/'support/lmdb/vendor')
    p.add_argument('--backend-init',type=Path);p.add_argument('--backend-init-provenance',type=Path)
    p.add_argument('--timeout',type=int,default=60)
    a=p.parse_args();require(a.timeout>0,'positive timeout required');require(bool(a.backend_init)==bool(a.backend_init_provenance),'initializer and provenance must be supplied together')
    run=Run(a)
    try:run.run()
    except BaseException as e:run.owned.cleanup_all(run.out);run.data['error']=type(e).__name__+': '+str(e);run.save();raise
    finally:run.owned.cleanup_all(run.out)
if __name__=='__main__':main()
