#!/usr/bin/env python3
"""Qualify real Linux buffered/mapped C durability with pinned public applications."""
from __future__ import annotations

import argparse
import collections
import hashlib
import itertools
import json
import os
from pathlib import Path
import re
import select
import shlex
import shutil
import signal
import subprocess
import sys
import time
import types

HERE = Path(__file__).resolve().parent
OWNED = types.ModuleType('durability_owned_processes')
OWNED.__file__ = str(HERE / 'processes.py')
exec(compile((HERE / 'processes.py').read_bytes(), str(HERE / 'processes.py'), 'exec'), OWNED.__dict__)
FROZEN = {
    'oracle.py': '80ed5f70218f0bce6a127e4f4f448f9185b79abe2518a1713380c0a430eb08dc',
    'probe.c': '70b64294261b8685554a2c58f773580a9d278ef8c7e7225331cf45e00097c955',
    'probe_existing.c': '0f8d707dd533a49b6d51aaab8b67a4dd7b3558ce17d04291a1a6457c52e8b290',
    'smoke.c': '7253b1a82b59ea10140a864bc14625d3de52287dafafbbc922cd42c4e5d038bc',
    'cases.json': '0768ace941caaa3cd05714415b7f7a3564bcfaec85740f46e194e8717b0b1d3f',
    'reference_pins.json': '8fd9c6db6666b6d224a2fd1d0f3ce8fed4c03144dae8ec10b1d10e98b177c22d',
    'retained_probe.c': '5787d955722078634877611e889eda9df26564902a6e5445fcd28613079a9840',
    'retained_contract.json': 'bcb3742b0f96ac4b382f6b9dc441fae7977e1293b69c4663a96e26535f0f0b37',
    'backend_init/main.rs': 'd3122fa934234dfff338080293fb81d4aa837704e28b108c9f8f41238a45c7e2',
    'mapped_oracle.py': 'b45623f462f3a65074cfd2977ea26dee3ac2c1badcc3d077a66bb1a6a8334ceb',
    'test_mapped_oracle.py': 'de6a0f29f2edc3abf69a19bddee2a579074277b5b21042e9e2a8ef10d2f598e5',
    'mapped_retry_probe.c': 'c50c8978db30d00b6f2a62aec86b60a3469c6315f85a0bf5814ef2df2a8c8d11',
}
EXTRAS = {
    'policy_fork.c': '67bfb7c7f89ab1214931191f3729bd7f7746acadbdae09843edeed63e8b0332d',
    'policy_copies.c': '50d0e180c8cb3ddb31d35d4f37a9e1e3cea85ba7634e11d3a5b7a67657ce4194',
    'copy_archives.c': 'c5c534bf90ea81a31bd2960b5d5f0ca4db87994550d0a5b99c0ddb9c27031c0c',
}
PINS = {'0.9.31': 'legacy-source.json', '0.9.36': 'current09-source.json', '1.0.2': 'modern-source.json'}
TRACE = 'openat,close,fsync,fdatasync,msync,pwrite64,pwritev,write,writev,lseek,mmap,munmap,mremap,ftruncate,rename,renameat,renameat2,unlink,unlinkat,fstat,newfstatat,statx'
SAMPLES = [
    ['full', '0', 'commit', 'full', 'full', 'commit'],
    ['nosync', '0', 'commit', 'full', 'full', 'commit'],
    ['nometasync', '0', 'commit', 'full', 'full', 'commit'],
    ['nosync', '0', 'runtime-before', 'full', 'full', 'commit'],
    ['nosync', '0', 'runtime-after-prepare', 'full', 'full', 'commit'],
    ['full', '0', 'runtime-after-prepare', 'full', 'nosync', 'commit'],
    ['full', '0', 'nested-clean', 'full', 'nosync', 'commit'],
    ['full', '0', 'rollback-prepared', 'nosync', 'full', 'commit'],
    ['full', '0', 'sync-dirty', 'full', 'full', 'commit'],
]
FAULT = ['full', '0', 'fault-sync', 'full', 'full', 'commit']


def digest(path: Path) -> str:
    with path.open('rb') as source:
        return hashlib.file_digest(source, 'sha256').hexdigest()


def require(yes: bool, message: str) -> None:
    if not yes:
        raise RuntimeError(message)


def find_root() -> Path:
    for path in HERE.parents:
        if (path / 'Cargo.toml').is_file() and (path / 'clients/lmdb-ulmdb').is_dir():
            return path
    raise RuntimeError('provide --source-root containing the reference pin manifests and C fixtures')


def library_at(path: Path) -> Path:
    return (path / 'lib/liblmdb.so.0' if path.is_dir() else path).resolve()


def normalized(data: bytes) -> bytes:
    result, count = re.subn(rb'published\.pages=\d+', b'published.pages=<physical-layout>', data)
    require(count == 1, 'exactly one declared physical page observation required')
    return result


def read_ready(process: subprocess.Popen, expected: bytes, timeout: float) -> bytes:
    """Bound even a provider that emits a partial line and then deadlocks."""
    deadline = time.monotonic() + timeout
    data = bytearray()
    try:
        while b'\n' not in data:
            remaining = deadline - time.monotonic()
            if remaining <= 0 or not select.select([process.stdout], [], [], remaining)[0]:
                raise TimeoutError('owned weak-copy ready deadline')
            chunk = os.read(process.stdout.fileno(), len(expected) + 1 - len(data))
            require(bool(chunk), 'weak-copy process ended before ready marker')
            data.extend(chunk)
            process._owned_stdout_prefix = bytes(data)
            require(len(data) <= len(expected), 'unexpected weak-copy stdout before ready marker')
        require(bytes(data) == expected, 'unexpected weak-copy ready marker')
    except BaseException as exc:
        exc.observed_stdout = bytes(data)
        raise
    return bytes(data)


class Qualification:
    def __init__(self, a: argparse.Namespace):
        self.a, self.out = a, a.output.resolve()
        self.out.mkdir(parents=True, exist_ok=False)
        self.cc = shlex.split(os.environ.get('CC', 'cc'))
        self.protected: dict[str, str] = {}
        self.generated: dict[str, str] = {}
        self.receipt = {'schema': 1, 'profile': 'lmdb-c-buffered-durability-v1', 'complete': False,
                        'passed': False, 'product_qualified': False, 'commands': 'commands.jsonl',
                        'command_count': 0, 'providers': {}, 'references': {}, 'applications': {},
                        'cases': [], 'controls': [], 'backend_samples': [], 'extra_controls': [],
                        'retained_EIO': [], 'retained_EIO_negatives': [],
                        'scope': 'Linux buffered/mapped ordinary file C policies; full314 mapped acceptance pending complete corpus and mandatory controls; power-loss/performance excluded',
                        'mapped_acceptance': {'status': 'pending', 'required_attempts': 314, 'passed_attempts': 0},
                        'host': {'uname': list(os.uname()), 'python': sys.version},
                        'normalization': 'Only published.pages integer; all other public flags/IDs/values/errors/mapsize/pagesize remain exact'}
        self.env = dict(os.environ)
        for name in tuple(self.env):
            if name.startswith(('ULMDB_', 'LD_')):
                self.env.pop(name)
        self.env.update(LD_BIND_NOW='1', LC_ALL='C')
        self.apps: dict[str, dict] = {}
        self.expected: dict[tuple[str, tuple[str, ...]], bytes] = {}
        self.sequence = 0
        (self.out / 'sources').mkdir()
        (self.out / 'commands').mkdir()
        self.save()

    def save(self) -> None:
        (self.out / 'receipt.json').write_text(json.dumps(self.receipt, indent=2) + '\n')

    def protect(self, path: Path) -> str:
        path = path.resolve()
        require(path.is_file(), f'missing required input: {path}')
        value = digest(path)
        previous = self.protected.setdefault(str(path), value)
        require(previous == value, f'input changed: {path}')
        return value

    def artifact(self, path: Path) -> None:
        self.generated[str(path.relative_to(self.out))] = digest(path)

    def copy(self, source: Path, target: Path) -> None:
        value = self.protect(source)
        target.parent.mkdir(parents=True, exist_ok=True)
        shutil.copyfile(source, target)
        require(digest(target) == value, f'input changed while copied: {source}')
        self.artifact(target)

    def command(self, label: str, argv: list, env: dict | None = None) -> subprocess.CompletedProcess:
        command = [str(x) for x in argv]
        self.sequence += 1
        stem = f'{self.sequence:05d}-{label}'
        started = time.monotonic()
        process = OWNED.start(command, env=env or self.env, stdout=subprocess.PIPE, stderr=subprocess.PIPE)
        failure = None
        try:
            stdout, stderr = process.communicate(timeout=self.a.timeout)
        except BaseException as exc:
            failure = exc
            with OWNED.ignore_interrupts():
                try:
                    stdout, stderr = OWNED.stop(process)
                except BaseException as cleanup:
                    stdout, stderr = getattr(cleanup, 'stdout', b''), getattr(cleanup, 'stderr', b'')
                    failure = cleanup
        # Keep this process registered until all result bytes are on disk. If a
        # signal lands here, main's fallback still owns the group and streams.
        out, err = self.out / 'commands' / (stem + '.stdout'), self.out / 'commands' / (stem + '.stderr')
        out.write_bytes(stdout)
        err.write_bytes(stderr)
        self.artifact(out)
        self.artifact(err)
        row = {'label': label, 'argv': command, 'pid': process.pid, 'returncode': process.returncode,
               'stdout': str(out.relative_to(self.out)), 'stderr': str(err.relative_to(self.out)),
               'stdout_sha256': digest(out), 'stderr_sha256': digest(err),
               'seconds_observed_not_performance': time.monotonic() - started,
               'environment': {k: v for k, v in (env or self.env).items() if k.startswith(('LD_', 'ULMDB_'))}}
        if failure:
            row['harness_failure'] = type(failure).__name__
        with (self.out / 'commands.jsonl').open('a') as stream:
            stream.write(json.dumps(row) + '\n')
        self.receipt['command_count'] = self.sequence
        if failure:
            with OWNED.ignore_interrupts():
                self.save()
            OWNED.forget(process)
            raise RuntimeError(f'{label}: owned process deadline/interruption is never an accepted API rejection') from failure
        OWNED.forget(process)
        return subprocess.CompletedProcess(command, process.returncode, stdout, stderr)

    def successful(self, label: str, argv: list) -> bytes:
        result = self.command(label, argv)
        require(result.returncode == 0, f'{label}: failed; inspect commands.jsonl and retained stderr')
        return result.stdout

    def source_setup(self) -> None:
        require(sys.platform == 'linux', 'this syscall qualification requires Linux; no platform skip')
        for tool in [self.cc[0], 'ld', 'objcopy', 'nm', 'readelf', 'strace']:
            require(shutil.which(tool) is not None, f'required tool is missing: {tool}')
        for name, expected in FROZEN.items():
            require(self.protect(HERE / name) == expected, f'frozen source changed: {name}')
        self.source_snapshot = {str(path.relative_to(HERE)): digest(path) for path in HERE.rglob('*')
            if path.is_file() and '__pycache__' not in path.parts and path.suffix in {'.py', '.c', '.rs', '.json', '.md'}}
        self.receipt['qualification_source_before'] = self.source_snapshot
        for name in sorted(self.source_snapshot):
            self.copy(HERE / name, self.out / 'sources' / name)
        self.oracle = types.ModuleType('frozen_durability_oracle')
        # Execute the verified bytes, never a potentially stale __pycache__.
        exec(compile((self.out / 'sources/oracle.py').read_bytes(), str(self.out / 'sources/oracle.py'), 'exec'), self.oracle.__dict__)
        self.mapped_oracle = types.ModuleType('frozen_mapped_durability_oracle')
        self.mapped_oracle.__file__ = str(self.out / 'sources/mapped_oracle.py')
        exec(compile((self.out / 'sources/mapped_oracle.py').read_bytes(), self.mapped_oracle.__file__, 'exec'), self.mapped_oracle.__dict__)
        self.retained_oracle = types.ModuleType('frozen_retained_eio_oracle')
        exec(compile((self.out / 'sources/retained_oracle.py').read_bytes(), str(self.out / 'sources/retained_oracle.py'), 'exec'), self.retained_oracle.__dict__)
        self.cases = json.loads((self.out / 'sources/cases.json').read_text())['cases']
        require(len(self.cases) == 640, 'frozen case inventory is incomplete')
        require(collections.Counter(c['version'] for c in self.cases) == {'0.9.31': 144, '0.9.36': 144, '1.0.2': 352}, 'wrong public-header case inventory')
        require(len(self.mapped_oracle.declared_cases()) == 314, 'mapped declaration must retain all attempts, including4 repeat rows')
        for label, argv in [('compiler', self.cc + ['--version']), ('linker', ['ld', '--version']),
                            ('objcopy', ['objcopy', '--version']), ('observer', ['strace', '-V'])]:
            self.successful('version-' + label, argv)

    def pins(self, version: str, source: Path) -> Path:
        source = source.resolve()
        if (source / 'libraries/liblmdb').is_dir():
            source = source / 'libraries/liblmdb'
        manifest = self.a.source_root / 'clients/lmdb-ulmdb' / PINS[version]
        self.protect(manifest)
        pins = json.loads(manifest.read_text())
        require(pins['ref'] == 'LMDB_' + version, 'wrong pinned reference version')
        canonical = json.loads((self.out / 'sources/reference_pins.json').read_text())['references'][version]
        require(pins == canonical, 'repository reference manifest differs from the frozen public contract')
        target = self.out / 'references' / version / 'source'
        target.mkdir(parents=True)
        for row in pins['files']:
            path = source / Path(row['path']).name
            require(path.is_file() and not path.is_symlink(), f'pinned source must be regular: {path}')
            require(path.stat().st_size == row['bytes'] and self.protect(path) == row['sha256'], f'pinned reference source mismatch: {path}')
            self.copy(path, target / path.name)
        self.receipt['references'][version] = {'pins': pins, 'manifest_sha256': digest(manifest), 'source': str(target)}
        return target

    def provider(self, name: str, source: Path) -> Path:
        target = self.out / 'providers' / name / 'liblmdb.so.0'
        self.copy(source, target)
        target.chmod(0o444)
        self.receipt['providers'][name] = {'path': str(target), 'sha256': digest(target), 'role': name}
        return target

    def exports(self, name: str, library: Path) -> set[str]:
        data = self.successful('exports-' + name, ['nm', '-D', '--defined-only', library]).decode()
        symbols = {line.split()[-1] for line in data.splitlines() if len(line.split()) >= 3}
        require(all(re.fullmatch(r'[A-Za-z_][A-Za-z0-9_]*', s) for s in symbols), 'unsupported versioned/nonidentifier export')
        return symbols

    def compile_app(self, name: str, source: Path, header: Path, library: Path) -> None:
        app = self.out / 'apps' / name
        app.parent.mkdir(exist_ok=True)
        self.successful('compile-' + name, self.cc + ['-std=c11', '-O2', '-Wall', '-Wextra', '-Werror', '-pthread',
                         '-I', header, source, '-L', library.parent, '-llmdb', '-o', app])
        dynamic = self.successful('dynamic-' + name, ['readelf', '-d', app]).decode()
        require('[liblmdb.so.0]' in dynamic and not re.search(r'\(NEEDED\).*\[/', dynamic), 'application must import only the reference SONAME basename')
        require('RPATH' not in dynamic and 'RUNPATH' not in dynamic, 'reference app must not hardcode its provider')
        imports = self.successful('imports-' + name, ['nm', '-D', '--undefined-only', app]).decode()
        names = sorted(set(re.findall(r'\b(mdb_\w+)\b', imports)))
        self.artifact(app)
        self.apps[name] = {'path': str(app), 'sha256': digest(app), 'imports': names}
        self.receipt['applications'][name] = self.apps[name]

    def app_env(self, library: Path, version: str) -> dict:
        return dict(self.env, LD_LIBRARY_PATH=str(library.parent),
                    ULMDB_LMDB_ABI='1.0' if version == '1.0.2' else version)

    def bindings(self, app: dict, library: Path, stderr: bytes) -> list[str]:
        text = stderr.decode(errors='strict')
        names = [name for name in app['imports'] if re.search(
            r'binding file ' + re.escape(app['path']) + r'.* to ' + re.escape(str(library))
            + r'.*symbol [\x27`]' + re.escape(name) + r"['`]", text)]
        require(names == app['imports'], 'application import did not bind to the selected actual provider')
        return names

    def run_app(self, label: str, app_name: str, library: Path, version: str, args: list[str],
                *, trace: bool = False, precreate: bool = False, backend: str | None = None,
                injection: str | None = None) -> tuple[subprocess.CompletedProcess, dict]:
        app = self.apps[app_name]
        path = Path(app['path'])
        require(digest(path) == app['sha256'], 'unchanged executable drifted')
        folder = self.out / 'runs' / label
        folder.mkdir(parents=True, exist_ok=False)
        database = folder / 'database'
        initial_assets = []
        if backend:
            self.successful(label + '-empty-init', [self.helper, backend, database])
            self.check_mode(database, backend)
            for directory in ['segments', 'commits', 'cohorts']:
                parent = database / directory
                if parent.exists():
                    for asset in sorted(parent.iterdir()):
                        if asset.suffix in {'.seg', '.root', '.cohort'}:
                            require(asset.is_file() and not asset.is_symlink(), 'initializer asset must be a regular immutable file')
                            initial_assets.append({'path': str(asset), 'sha256': digest(asset)})
        elif precreate:
            database.mkdir(mode=0o700)
        env = self.app_env(library, version)
        loader = self.command(label + '-loader', [path], dict(env, LD_TRACE_LOADED_OBJECTS='1'))
        require(loader.returncode == 0 and str(library) in loader.stdout.decode(), 'loader selected an unexpected provider')
        command: list = [path, database] + args
        if trace:
            command = ['strace', '-f', '-v', '-qq', '-yy', '-ttt', '-s', '4096', '-e', 'trace=' + TRACE,
                       '-o', folder / 'syscalls.trace'] + (['-e', 'inject=' + injection] if injection else []) + command
        result = self.command(label, command, dict(env, LD_DEBUG='libs,bindings'))
        (folder / 'stdout').write_bytes(result.stdout)
        (folder / 'stderr').write_bytes(result.stderr)
        self.artifact(folder / 'stdout')
        self.artifact(folder / 'stderr')
        bound = self.bindings(app, library, result.stderr)
        row = {'label': label, 'path': str(folder), 'version': version, 'arguments': args,
               'provider_sha256': digest(library), 'executable_sha256': app['sha256'],
               'returncode': result.returncode, 'stdout_sha256': digest(folder / 'stdout'),
               'stderr_sha256': digest(folder / 'stderr'), 'actual_bindings': bound}
        if trace:
            self.artifact(folder / 'syscalls.trace')
            row['trace_sha256'] = digest(folder / 'syscalls.trace')
        if backend:
            row['backend'] = backend
            row['selected_magic'] = self.check_mode(database, backend)
            row['initializer_committed_assets'] = initial_assets
            require(all(digest(Path(item['path'])) == item['sha256'] for item in initial_assets), 'initializer committed asset changed during C calls')
        require(digest(path) == app['sha256'], 'application changed during execution')
        return result, row

    def check_mode(self, directory: Path, backend: str) -> str:
        current = (directory / 'CURRENT').read_bytes()
        require(len(current) in {64, 144, 176}, 'unknown actual native selector shape')
        pointer = current if len(current) == 64 else current[16:80]
        expected = {'legacy': b'ULMDBCUR', 'cohort': b'ULMDBC03', 'indexed': b'ULMDBT04'}[backend]
        require(pointer[:8] == expected, f'actual backend differs: requested {backend}')
        return pointer[:8].decode()

    def validate(self, row: dict, *, mapped: bool = False) -> dict:
        folder = Path(row['path'])
        require(mapped == (row['arguments'][1] == '1'), 'wrong buffered/mapped observer route')
        observer = self.mapped_oracle if mapped else self.oracle
        parsed = observer.parse(folder / 'syscalls.trace', folder / 'database')
        report = observer.validate(row['arguments'], parsed, row['version'])
        require(report['passed'], 'fence oracle did not pass explicitly')
        target = folder / 'fence_report.json'
        target.write_text(json.dumps(report, indent=2) + '\n')
        self.artifact(target)
        return {'status': 'passed-mapped' if mapped else 'passed-buffered', 'report': str(target.relative_to(self.out)), 'sha256': digest(target)}

    def helper_provenance(self) -> None:
        manifest = self.a.backend_init_provenance.resolve()
        self.protect(manifest)
        data = json.loads(manifest.read_text())
        require(data.get('schema') == 1 and data.get('role') == 'empty-native-backend-initializer'
                and data.get('complete') and data.get('passed'), 'backend initializer provenance is incomplete')
        self.helper = self.out / 'providers' / 'empty-native-init'
        self.copy(self.a.backend_init, self.helper)
        self.helper.chmod(0o555)
        require(digest(self.helper) == data['executable_sha256'], 'backend initializer executable/provenance mismatch')
        require(data.get('entry_source_sha256') == FROZEN['backend_init/main.rs'], 'initializer must be the frozen empty-only helper')
        require(bool(data.get('sources')) and bool(data.get('commands')), 'initializer source/build evidence required')
        require(data.get('source_before_after_equal') is True, 'initializer source consistency was not established')
        require(all(row.get('returncode') == 0 and not row.get('timeout') and not row.get('harness_failure')
                    and not row.get('interrupted') and not row.get('error')
                    for row in data['commands']), 'initializer provenance contains failed/incomplete build commands')
        observed_entry = False
        for source in data['sources']:
            path = Path(source['path'])
            if not path.is_absolute():
                path = manifest.parent / path
            value = self.protect(path)
            require(value == source['sha256'], 'initializer source changed since its build evidence')
            observed_entry |= value == FROZEN['backend_init/main.rs']
        require(observed_entry, 'initializer entry source was not included in source provenance')
        self.receipt['backend_initializer'] = {'manifest': str(manifest), 'manifest_sha256': digest(manifest),
            'executable_sha256': digest(self.helper), 'source_rows_verified': len(data['sources']),
            'qualification_inherited': False,
            'meaning': 'only empty seq0 physical modes; every measured business call runs through the actual C provider'}

    def references(self) -> None:
        sources = {'0.9.31': self.a.legacy_source, '0.9.36': self.a.current09_source, '1.0.2': self.a.lmdb_source}
        self.headers, self.refs = {}, {}
        for version, source in sources.items():
            header = self.pins(version, source)
            directory = self.out / 'providers' / ('reference-' + version)
            directory.mkdir(parents=True)
            library = directory / 'liblmdb.so.0'
            self.successful('build-reference-' + version, self.cc + ['-std=c11', '-O2', '-D_GNU_SOURCE', '-fPIC', '-shared',
                '-pthread', '-Wl,-soname,liblmdb.so.0', '-I', header, header / 'mdb.c', header / 'midl.c', '-o', library])
            (directory / 'liblmdb.so').symlink_to('liblmdb.so.0')
            self.artifact(library)
            self.headers[version], self.refs[version] = header, library
            self.receipt['references'][version]['provider_sha256'] = digest(library)
            self.compile_app('probe-' + version, self.out / 'sources/probe.c', header, library)
            self.compile_app('mapped-retry-' + version, self.out / 'sources/mapped_retry_probe.c', header, library)
        self.compile_app('existing', self.out / 'sources/probe_existing.c', self.headers['1.0.2'], self.refs['1.0.2'])
        self.compile_app('smoke', self.out / 'sources/smoke.c', self.headers['1.0.2'], self.refs['1.0.2'])
        self.compile_app('retained', self.out / 'sources/retained_probe.c', self.headers['1.0.2'], self.refs['1.0.2'])

    def corpus(self, package: Path) -> None:
        counters = collections.Counter()
        for declaration_index, case in enumerate(self.cases):
            version, args = case['version'], case['arguments']
            index = counters[version]
            counters[version] += 1
            reference, refrow = self.run_app(f'reference-{version}-{index:03d}', 'probe-' + version, self.refs[version], version, args)
            require(reference.returncode == 0, 'pinned reference rejected a frozen valid case')
            self.expected[(version, tuple(args))] = reference.stdout
            result, row = self.run_app(f'package-{version}-{index:03d}', 'probe-' + version, package, version, args, trace=True)
            require(result.returncode == 0 and normalized(result.stdout) == normalized(reference.stdout),
                    'actual packaged provider public trace differs from pinned reference: ' + row['label'])
            row['reference'] = refrow
            row['declaration_index'] = declaration_index
            row['fences'] = self.validate(row, mapped=args[1] == '1')
            self.receipt['cases'].append(row)
            self.receipt['mapped_acceptance']['passed_attempts'] += int(args[1] == '1')
            self.save()
        counts = collections.Counter(row['fences']['status'] for row in self.receipt['cases'])
        require(counts == {'passed-buffered': 326, 'passed-mapped': 314}, 'incomplete buffered/mapped case accounting')
        require([row['declaration_index'] for row in self.receipt['cases']] == list(range(640)), 'declared execution omitted/repeated')
        require(all(row['version'] == case['version'] and row['arguments'] == case['arguments'] for row, case in zip(self.receipt['cases'], self.cases)), 'declared execution order/shape drift')
        self.receipt['corpus_counts'] = dict(counts)
        self.receipt['mapped_acceptance']['status'] = 'complete-corpus-controls-pending'

    def backend_samples(self, package: Path) -> None:
        for index, args in enumerate(SAMPLES):
            reference, _ = self.run_app(f'existing-reference-{index:02d}', 'existing', self.refs['1.0.2'], '1.0.2', args, precreate=True)
            require(reference.returncode == 0 and normalized(reference.stdout) == normalized(self.expected[('1.0.2', tuple(args))]),
                    'EEXIST-only source variant changed the reference public contract')
            for backend in ['legacy', 'cohort', 'indexed']:
                result, row = self.run_app(f'backend-{backend}-{index:02d}', 'existing', package, '1.0.2', args,
                                           trace=True, backend=backend)
                require(result.returncode == 0 and normalized(result.stdout) == normalized(reference.stdout), 'actual backend public trace mismatch')
                row['fences'] = self.validate(row)
                self.receipt['backend_samples'].append(row)
                self.save()
        require(len(self.receipt['backend_samples']) == 27, 'missing real-mode fence samples')

    def inventory_setup(self, package: Path) -> None:
        header = re.sub(r'/\*.*?\*/', '', (self.headers['0.9.31'] / 'lmdb.h').read_text(), flags=re.S)
        header = re.sub(r'^\s*#.*$', '', header, flags=re.M)
        common = set()
        for declaration in header.split(';'):
            match = re.search(r'\b(mdb_[A-Za-z0-9_]+)\s*\([^;{}]*\)\s*$', declaration)
            if match:
                common.add(match.group(1))
        require(len(common) == 56, 'pinned common56 inventory parser changed')
        self.original_exports = self.exports('package', package)
        require(common <= self.original_exports and {'mdb_txn_prepare', 'mdb_env_rollback'} <= self.original_exports,
                'required APIs missing; no silent skip')
        self.receipt['providers']['package']['exports'] = sorted(self.original_exports)
        source = self.out / 'sources/common56.c'
        names = ',\n'.join(json.dumps(n) for n in sorted(common))
        source.write_text('#define _GNU_SOURCE\n#include <dlfcn.h>\n#include <stdio.h>\n'
            'int main(int argc,char**argv){if(argc!=2)return 2;void*h=dlopen(argv[1],RTLD_NOW|RTLD_LOCAL|RTLD_NODELETE);if(!h)return 1;'
            'const char*n[]={' + names + '};void*first=0;for(unsigned i=0;i<56;i++){void*p=dlsym(h,n[i]);if(!p)return 1;if(!i)first=p;}'
            'Dl_info d;if(!dladdr(first,&d)||!d.dli_fname)return 1;fprintf(stderr,"provider=%s\\n",d.dli_fname);puts("common56=56");return 0;}\n')
        app = self.out / 'apps/common56'
        self.successful('compile-common56', self.cc + ['-std=c11', '-O2', '-Wall', '-Wextra', '-Werror', source, '-ldl', '-o', app])
        self.artifact(source)
        self.artifact(app)
        self.common56 = common
        reference, _ = self.run_app('smoke-reference', 'smoke', self.refs['1.0.2'], '1.0.2', [], precreate=True)
        require(reference.returncode == 0, 'pinned common smoke failed')
        self.smoke_expected = reference.stdout

    def smoke(self, name: str, library: Path) -> None:
        inventory = self.command('inventory-' + name, [self.out / 'apps/common56', library])
        require(inventory.returncode == 0 and inventory.stdout == b'common56=56\n'
                and inventory.stderr == ('provider=' + str(library) + '\n').encode(), 'unrelated common56 inventory/provider failed')
        result, row = self.run_app('smoke-' + name, 'smoke', library, '1.0.2', [], precreate=True)
        require(result.returncode == 0 and result.stdout == self.smoke_expected, 'unrelated common smoke changed: ' + name)
        self.receipt['providers'][name]['unrelated_smoke'] = row
        self.receipt['providers'][name]['common56'] = sorted(self.common56)
        self.save()

    def relink(self, name: str, archive: Path, noop: bool = False) -> Path:
        folder = self.out / 'providers' / name
        folder.mkdir(parents=True)
        script = folder / 'exports.map'
        script.write_text('{\n global:\n' + ''.join('  ' + n + ';\n' for n in sorted(self.original_exports)) + ' local: *;\n};\n')
        library = folder / 'liblmdb.so.0'
        command: list = self.cc + ['-shared', '-Wl,-soname,liblmdb.so.0', '-Wl,--version-script=' + str(script)]
        if noop:
            changed = folder / 'renamed-archive.a'
            self.successful('rename-sync-export', ['objcopy', '--redefine-sym', 'mdb_env_sync=ulmdb_real_env_sync', archive, changed])
            self.artifact(changed)
            archive = changed
            source = folder / 'noop_sync.c'
            source.write_text('#include <lmdb.h>\nextern int ulmdb_real_env_sync(MDB_env*,int);\n'
                '/* Genuine deficient public entrypoint; preserve invalid/readonly errors. */\n'
                'int mdb_env_sync(MDB_env*env,int force){unsigned flags=0;const char*path=0;'
                'if(mdb_env_get_flags(env,&flags)||mdb_env_get_path(env,&path)||!path||(flags&MDB_RDONLY))'
                'return ulmdb_real_env_sync(env,force);(void)force;return 0;}\n')
            obj = folder / 'noop_sync.o'
            self.successful('compile-noop-sync', self.cc + ['-std=c11', '-O2', '-fPIC', '-Wall', '-Wextra', '-Werror',
                            '-I', self.headers['1.0.2'], '-c', source, '-o', obj])
            command.append(obj)
            self.artifact(source)
            self.artifact(obj)
        command += ['-Wl,--whole-archive', archive, '-Wl,--no-whole-archive', '-ldl', '-lpthread', '-lm', '-lrt', '-lutil', '-o', library]
        self.successful('link-' + name, command)
        require(self.exports(name, library) == self.original_exports, 'relink changed unrelated export inventory')
        self.artifact(script)
        self.artifact(library)
        self.receipt['providers'][name] = {'path': str(library), 'sha256': digest(library), 'exports': sorted(self.original_exports), 'role': name}
        return library

    def sample_provider(self, name: str, library: Path, *, deficient: bool = False, historical: bool = False) -> None:
        for index, args in enumerate(SAMPLES):
            result, row = self.run_app(f'{name}-sample-{index:02d}', 'probe-1.0.2', library, '1.0.2', args, trace=True)
            require(result.returncode == 0 and normalized(result.stdout) == normalized(self.expected[('1.0.2', tuple(args))]),
                    name + ': API trace changed; unrelated failure is not an accepted deficient-fence rejection')
            if deficient or historical:
                folder = Path(row['path'])
                try:
                    self.oracle.validate(args, self.oracle.parse(folder / 'syscalls.trace', folder / 'database'), '1.0.2')
                except self.oracle.ValidationError as exc:
                    require(not isinstance(exc, self.oracle.UnsupportedScope), 'deficient provider rejected only by scope')
                    if deficient:
                        require('active selected sync fences' in str(exc), 'no-op sync was rejected by an unrelated fence condition')
                    row.update(rejected=True, reason=str(exc))
                else:
                    raise RuntimeError(name + ': deficient actual provider was silently accepted')
            else:
                row['fences'] = self.validate(row)
            self.receipt['controls'].append({'kind': name, 'run': row})
            self.save()

    def observer_controls(self) -> None:
        row = next(r for r in self.receipt['cases'] if r['arguments'] == SAMPLES[0] and r['version'] == '1.0.2')
        original = Path(row['path']) / 'syscalls.trace'
        lines = original.read_text().splitlines(keepends=True)
        for kind in ['remove-force-fences', 'remove-required-phase']:
            changed, active, removed = [], False, 0
            for line in lines:
                begin, end = '@@BEGIN sync.after.force1@@' in line, '@@END sync.after.force1@@' in line
                if begin:
                    active = True
                drop = active and bool(re.search(r'\b(?:fsync|fdatasync|msync)\(', line)) if kind == 'remove-force-fences' else begin or end
                if drop:
                    removed += 1
                else:
                    changed.append(line)
                if end:
                    active = False
            require(removed >= (3 if kind == 'remove-force-fences' else 2), 'observer mutation did not remove required evidence')
            path = self.out / (kind + '.trace')
            path.write_text(''.join(changed))
            self.artifact(path)
            try:
                self.oracle.validate(row['arguments'], self.oracle.parse(path, Path(row['path']) / 'database'), row['version'])
            except self.oracle.ValidationError as exc:
                self.receipt['controls'].append({'kind': kind, 'rejected': True, 'removed_lines': removed,
                    'source_trace_sha256': digest(original), 'mutant_trace_sha256': digest(path), 'reason': str(exc)})
            else:
                raise RuntimeError('observer with missing evidence was accepted')

    def eio_controls(self, package: Path, noop: Path) -> None:
        def expected_rejection(result: subprocess.CompletedProcess) -> bool:
            diagnostics = [line for line in result.stderr.splitlines() if line.startswith(b'oracle:')]
            return (result.returncode == 1 and result.stdout.endswith(b'sync.fault.force1=0\n')
                    and diagnostics == [b'oracle: injected sync EIO'])
        calibration, row = self.run_app('force-eio-calibration', 'probe-1.0.2', package, '1.0.2', FAULT, trace=True)
        require(expected_rejection(calibration), 'force-EIO calibration failed before its exact expected assertion')
        path = Path(row['path'])
        parsed = self.oracle.parse(path / 'syscalls.trace', path / 'database')
        calls = [e for e in parsed['events'] if e['syscall'] == 'fsync']
        target = next((e for e in calls if e['phase'] == 'sync.fault.force1' and e['role'] in {'descriptor', 'segment', 'cohort', 'current'}), None)
        require(target is not None and target['success'], 'force-sync performs no actual selected-file fsync')
        ordinal = calls.index(target) + 1
        injected, actual = self.run_app('force-eio-injected', 'probe-1.0.2', package, '1.0.2', FAULT, trace=True,
                                      injection=f'fsync:error=EIO:when={ordinal}')
        folder = Path(actual['path'])
        events = self.oracle.parse(folder / 'syscalls.trace', folder / 'database')
        failures = [e for e in events['phases']['sync.fault.force1'] if e['syscall'] == 'fsync' and not e['success']]
        require(injected.returncode == 0 and injected.stdout.endswith(b'sync.fault.force1=5\nsync-error.writer=usable\n'),
                'actual sync EIO was not returned with subsequent new-writer admission')
        require(len(failures) == 1 and failures[0]['role'] in {'descriptor', 'segment', 'cohort', 'current'}
                and failures[0]['result'].startswith('-1 EIO ') and '(INJECTED)' in failures[0]['result'], 'EIO did not hit intended selected force-sync fence')
        require((folder / 'syscalls.trace').read_text().count('(INJECTED)') == 1, 'unexpected extra I/O injection')
        self.receipt['controls'].append({'kind': 'actual-force-sync-EIO', 'passed': True, 'ordinal': ordinal, 'calibration': row, 'run': actual, 'failed_fence': failures[0],
            'scope': 'no writer active during EIO; proves error propagation and subsequent NEW-writer admission'})
        no_calibration, norow = self.run_app('noop-force-eio-calibration', 'probe-1.0.2', noop, '1.0.2', FAULT, trace=True)
        nofolder = Path(norow['path'])
        noparsed = self.oracle.parse(nofolder / 'syscalls.trace', nofolder / 'database')
        require(expected_rejection(no_calibration), 'no-op force EIO oracle rejected for an unrelated reason')
        phase = noparsed['phases']['sync.fault.force1']
        require(not any(e['syscall'] in {'fsync', 'fdatasync', 'msync'} for e in phase), 'no-op control unexpectedly performed a fence')
        # Markers are intentionally not ordinary events. Count fsync calls from
        # the raw prefix ending at the verified BEGIN marker.
        prefix = (nofolder / 'syscalls.trace').read_text().split('@@BEGIN sync.fault.force1@@', 1)[0]
        next_ordinal = len(re.findall(r'\bfsync\(', prefix)) + 1
        rejected, rerow = self.run_app('noop-force-eio-injected', 'probe-1.0.2', noop, '1.0.2', FAULT, trace=True,
                                      injection=f'fsync:error=EIO:when={next_ordinal}')
        refile = Path(rerow['path']) / 'syscalls.trace'
        require(expected_rejection(rejected) and '(INJECTED)' not in refile.read_text(),
                'no-op provider did not fail the exact force-I/O oracle after successful seed calls')
        self.receipt['controls'].append({'kind': 'noop-force-sync-EIO-oracle', 'passed': True, 'rejected': True,
             'scheduled_ordinal': next_ordinal, 'calibration': norow, 'run': rerow,
             'meaning': 'all prior seed calls succeed; no force fence executes, return0 violates the unchanged required EIO assertion'})
        self.save()

    def relink_no_mapping(self, archive: Path) -> Path:
        """Real full Rust implementation, deliberately buffered while advertising WRITEMAP."""
        folder = self.out / 'providers/no-mapping'
        folder.mkdir(parents=True)
        changed = folder / 'renamed-archive.a'
        self.successful('rename-mapping-exports', ['objcopy', '--redefine-sym', 'mdb_env_open=ulmdb_real_env_open',
            '--redefine-sym', 'mdb_env_get_flags=ulmdb_real_env_get_flags', archive, changed])
        self.artifact(changed)
        source = folder / 'no_mapping.c'
        source.write_text('#include <lmdb.h>\n'
            'extern int ulmdb_real_env_open(MDB_env*,const char*,unsigned,mdb_mode_t);'
            'extern int ulmdb_real_env_get_flags(MDB_env*,unsigned*);'
            'static MDB_env*owner;static unsigned advertised;\n'
            'int mdb_env_open(MDB_env*e,const char*p,unsigned f,mdb_mode_t m){owner=e;advertised=f&MDB_WRITEMAP;'
            'return ulmdb_real_env_open(e,p,f&~MDB_WRITEMAP,m);}\n'
            'int mdb_env_get_flags(MDB_env*e,unsigned*f){int rc=ulmdb_real_env_get_flags(e,f);'
            'if(!rc&&e==owner)*f|=advertised;return rc;}\n')
        obj, library, exports = folder / 'no_mapping.o', folder / 'liblmdb.so.0', folder / 'exports.map'
        self.successful('compile-no-mapping', self.cc + ['-std=c11', '-O2', '-fPIC', '-Wall', '-Wextra', '-Werror',
            '-I', self.headers['1.0.2'], '-c', source, '-o', obj])
        exports.write_text('{\n global:\n' + ''.join('  ' + name + ';\n' for name in sorted(self.original_exports)) + ' local: *;\n};\n')
        self.successful('link-no-mapping', self.cc + ['-shared', '-Wl,-soname,liblmdb.so.0',
            '-Wl,--version-script=' + str(exports), obj, '-Wl,--whole-archive', changed,
            '-Wl,--no-whole-archive', '-ldl', '-lpthread', '-lm', '-lrt', '-lutil', '-o', library])
        require(self.exports('no-mapping', library) == self.original_exports, 'no-mapping control changed export inventory')
        for path in (source, obj, exports, library):
            self.artifact(path)
        self.receipt['providers']['no-mapping'] = {'path': str(library), 'sha256': digest(library),
            'exports': sorted(self.original_exports), 'role': 'real full buffered implementation advertising mapped flag'}
        return library

    def mapped_controls(self, package: Path, archive: Path, noop: Path) -> None:
        self.receipt['mapped_controls'] = []
        self.receipt['mapped_backend_samples'] = []
        args = ['full', '1', 'commit', 'full', 'full', 'commit']
        row = next(row for row in self.receipt['cases'] if row['version'] == '1.0.2' and row['arguments'] == args)
        folder = Path(row['path'])
        cohort_folder = None
        for backend in ('cohort', 'indexed'):
            result, sample = self.run_app('mapped-backend-' + backend, 'existing', package, '1.0.2', args,
                                         trace=True, backend=backend)
            require(result.returncode == 0 and normalized(result.stdout) == normalized(self.expected[('1.0.2', tuple(args))]),
                    'mapped backend public contract differs')
            sample['fences'] = self.validate(sample, mapped=True)
            self.receipt['mapped_backend_samples'].append(sample)
            if backend == 'cohort':
                cohort_folder = Path(sample['path'])
        require(cohort_folder is not None, 'real Cohort mapped control source missing')
        output = self.out / 'mapped-observer-controls'
        self.successful('mapped-observer-controls', [sys.executable, self.out / 'sources/test_mapped_oracle.py',
            '--trace', folder / 'syscalls.trace', '--database', folder / 'database',
            '--cohort-trace', cohort_folder / 'syscalls.trace', '--cohort-database', cohort_folder / 'database',
            '--output', output])
        observed = json.loads((output / 'receipt.json').read_text())
        require(observed['complete'] and observed['passed'] and len(observed['controls']) == 16,
                'mapped observer controls incomplete')
        for path in output.iterdir():
            self.artifact(path)
        self.receipt['mapped_controls'].append({'kind': 'observer-controls', 'count': 16,
            'receipt': str((output / 'receipt.json').relative_to(self.out)), 'sha256': digest(output / 'receipt.json')})
        no_mapping = self.relink_no_mapping(archive)
        self.smoke('no-mapping', no_mapping)
        for name, library, reason in [('no-mapping', no_mapping, 'no actual writer backing'),
                                      ('no-sync', noop, 'active selected sync fences')]:
            result, actual = self.run_app('mapped-deficient-' + name, 'probe-1.0.2', library, '1.0.2', args, trace=True)
            require(result.returncode == 0 and normalized(result.stdout) == normalized(self.expected[('1.0.2', tuple(args))]),
                    'mapped deficient provider failed unrelated public contract: ' + name)
            path = Path(actual['path'])
            try:
                self.mapped_oracle.validate(args, self.mapped_oracle.parse(path / 'syscalls.trace', path / 'database'), '1.0.2')
            except self.mapped_oracle.ValidationError as error:
                require(not isinstance(error, self.mapped_oracle.UnsupportedScope) and reason in str(error),
                        'mapped deficient provider rejected for unrelated reason: ' + str(error))
                actual.update(rejected=True, reason=str(error))
            else:
                raise RuntimeError('real deficient mapped provider accepted: ' + name)
            self.receipt['mapped_controls'].append({'kind': name, 'run': actual})
        # MAPASYNC leaves data debt. Force must reach a real selected-file fence
        # and return the actual I/O error through the unchanged public probe.
        fault = ['mapasync', '1', 'fault-sync', 'full', 'full', 'commit']
        result, calibration = self.run_app('mapped-force-eio-calibration', 'probe-1.0.2', package,
            '1.0.2', fault, trace=True)
        require(result.returncode == 1 and result.stdout.endswith(b'sync.fault.force1=0\n')
                and b'oracle: injected sync EIO\n' in result.stderr, 'mapped force calibration wrong failure')
        path = Path(calibration['path'])
        parsed = self.mapped_oracle.parse(path / 'syscalls.trace', path / 'database')
        calls = [e for e in parsed['events'] if e['syscall'] == 'fsync']
        target = next((e for e in calls if e['phase'] == 'sync.fault.force1' and e['role'] in self.mapped_oracle.ASSETS | {'current'}), None)
        require(target is not None and target['success'], 'mapped force skipped real selected-file fence')
        ordinal = calls.index(target) + 1
        result, actual = self.run_app('mapped-force-eio-injected', 'probe-1.0.2', package,
            '1.0.2', fault, trace=True, injection=f'fsync:error=EIO:when={ordinal}')
        path = Path(actual['path'])
        report = self.mapped_oracle.validate_eio(self.mapped_oracle.parse(path / 'syscalls.trace', path / 'database'),
            'sync.fault.force1', 'fsync')
        require(result.returncode == 0 and result.stdout.endswith(b'sync.fault.force1=5\nsync-error.writer=usable\n'),
                'mapped force EIO/new-writer response changed')
        require((path / 'syscalls.trace').read_text().count('(INJECTED)') == 1, 'mapped force injected unrelated I/O')
        self.receipt['mapped_controls'].append({'kind': 'mapped-force-EIO', 'calibration': calibration,
            'run': actual, 'report': report, 'scope': 'error propagation + subsequent NEW-writer admission; no retained writer here'})
        self.save()

    def mapped_retry_controls(self, package: Path) -> None:
        """Separate proper reference/candidate data-error lifetime contract."""
        self.receipt['mapped_retry'] = []
        expected = {}
        for version in ('0.9.31', '0.9.36', '1.0.2'):
            for async_flag in (0, 1):
                ordinal = None
                for injected in (False, True):
                    label = f'mapped-retry-reference-{version}-{async_flag}-{int(injected)}'
                    result, row = self.run_app(label, 'mapped-retry-' + version, self.refs[version], version,
                        [str(async_flag), str(int(injected))], trace=True,
                        injection=f'msync:error=EIO:when={ordinal}' if injected else None)
                    require(result.returncode == 0, 'pinned mapped retry fixture failed: ' + label)
                    path = Path(row['path'])
                    parsed = self.oracle.parse(path / 'syscalls.trace', path / 'database')
                    calls = [e for e in parsed['events'] if e['syscall'] == 'msync']
                    if not injected:
                        target = next((e for e in calls if e['phase'] == 'B.finish'), None)
                        require(target is not None and target['success'], 'pinned retry has no actual data msync')
                        ordinal = calls.index(target) + 1
                    else:
                        require((path / 'syscalls.trace').read_text().count('(INJECTED)') == 1
                            and any(not e['success'] and e['result'].startswith('-1 EIO ') for e in parsed['phases']['B.finish']),
                            'pinned mapped retry did not hit the actual data error')
                    wanted = (b'B.commit=5\nselected=1 old.borrow=A old.cursor=A\nnext.begin=0\nnext.id=2\nnext.commit=0\ncold.id=2 cold.value=C\nlifetime=complete\n' if injected else
                              b'B.commit=0\nselected=2 old.borrow=A old.cursor=A\nnext.begin=0\nnext.id=3\nnext.commit=0\ncold.id=3 cold.value=C\nlifetime=complete\n')
                    require(result.stdout == wanted, 'pinned mapped retry lifetime/reference boundary changed')
                    require(expected.setdefault((async_flag, injected), result.stdout) == result.stdout,
                            'pinned mapped retry behavior differs across references')
                    self.receipt['mapped_retry'].append({'kind': 'reference', 'async': async_flag, 'injected': injected, 'run': row})
                    self.save()
        for backend in ('legacy', 'cohort', 'indexed'):
            for async_flag in (0, 1):
                ordinal = None
                for injected in (False, True):
                    result, row = self.run_app(f'mapped-retry-{backend}-{async_flag}-{int(injected)}',
                        'mapped-retry-1.0.2', package, '1.0.2', [str(async_flag), str(int(injected))],
                        trace=True, backend=backend, injection=f'msync:error=EIO:when={ordinal}' if injected else None)
                    require(result.returncode == 0 and result.stdout == expected[(async_flag, injected)],
                            'actual mapped retry differs from unchanged pinned executable: ' + row['label'])
                    path = Path(row['path'])
                    parsed = self.mapped_oracle.parse(path / 'syscalls.trace', path / 'database')
                    calls = [e for e in parsed['events'] if e['syscall'] == 'msync']
                    if not injected:
                        target = next((e for e in calls if e['phase'] == 'B.finish'), None)
                        require(target is not None and target['success'] and target.get('mapping', {}).get('shared')
                            and target['mapping']['writable'], 'retry did not use actual shared writable data mapping')
                        require(target['args'].endswith('MS_ASYNC' if async_flag else 'MS_SYNC'), 'retry flush mode mismatch')
                        ordinal = calls.index(target) + 1
                    else:
                        report = self.mapped_oracle.validate_eio(parsed, 'B.finish', 'msync')
                        require((path / 'syscalls.trace').read_text().count('(INJECTED)') == 1, 'retry injected unrelated I/O')
                        target = path / 'mapped_error_report.json'
                        target.write_text(json.dumps(report, indent=2) + '\n')
                        self.artifact(target)
                        row['error_report_sha256'] = digest(target)
                    self.receipt['mapped_retry'].append({'kind': 'candidate', 'backend': backend,
                        'async': async_flag, 'injected': injected, 'run': row})
                    self.save()
        require(collections.Counter(row['kind'] for row in self.receipt['mapped_retry']) == {'reference': 12, 'candidate': 12},
                'mapped lifetime reference/candidate inventory incomplete')

    def extra_controls(self, package: Path) -> None:
        for name, expected in EXTRAS.items():
            source = self.a.source_root / 'clients/c/tests' / name
            require(self.protect(source) == expected, 'frozen adapter ownership/copy source changed: ' + name)
            self.copy(source, self.out / 'sources' / name)
        self.compile_app('fork', self.out / 'sources/policy_fork.c', self.headers['1.0.2'], self.refs['1.0.2'])
        self.compile_app('copies', self.out / 'sources/policy_copies.c', self.headers['1.0.2'], self.refs['1.0.2'])
        self.compile_app('archives', self.out / 'sources/copy_archives.c', self.headers['1.0.2'], self.refs['1.0.2'])
        for nolock in [0, 1]:
            for nosync in [0, 1]:
                for force in [0, 1]:
                    result, row = self.run_app(f'creator-{nolock}-{nosync}-{force}', 'fork', package, '1.0.2',
                                              [str(nolock), str(nosync), str(force)])
                    require(result.returncode == 0 and b'parent.writer.begin=0\nparent.retained_writer.sync.force1=0\ncreator.progress=ok\n' in result.stdout,
                            'inherited creator must reject before fastskip/NOLOCK minting; parent must progress')
                    self.receipt['extra_controls'].append({'kind': 'creator', 'scope': 'adapter-only, not valid inherited LMDB usage/G19 promotion', 'run': row})
                    self.save()
        for backend in ['legacy', 'cohort', 'indexed']:
            self.weak_copy(backend, package)
            self.archive_copy_contract(backend, package)

    def retained_case(self, label: str, library: Path, mode: str, state: str, policy: str, ordinal: int | None = None):
        result, row = self.run_app(label, 'retained', library, '1.0.2', [state, policy], trace=True, backend=mode,
                                  injection=None if ordinal is None else f'fsync:error=EIO:when={ordinal}')
        row.update(state=state, policy=policy, injected_fsync_ordinal=ordinal)
        folder = Path(row['path'])
        parsed = self.oracle.parse(folder / 'syscalls.trace', folder / 'database')
        (folder / 'retained_parsed.json').write_text(json.dumps(parsed, indent=2) + '\n')
        self.artifact(folder / 'retained_parsed.json')
        return result, row, parsed

    def retained_controls(self, package: Path, noop: Path, historical: Path | None) -> None:
        contract = json.loads((self.out / 'sources/retained_contract.json').read_text())
        plan = contract['case_plan']
        require(plan['positive_injected_cases'] == plan['calibration_cases'] == 27, 'retained-EIO contract grid changed')
        cases = list(itertools.product(plan['file_modes'], plan['writer_states'], plan['policies']))
        require(len(cases) == 27, 'retained-EIO grid incomplete')
        for index, (mode, state, policy) in enumerate(cases):
            result, row, parsed = self.retained_case(f'retained-{index:02d}-calibration', package, mode, state, policy)
            target = self.retained_oracle.calibration(self.oracle, row, result.stdout, result.stderr, parsed, True)
            record = {'backend': mode, 'state': state, 'policy': policy, 'calibration': row, 'target': target}
            self.receipt['retained_EIO'].append(record)
            self.save()
            result, actual, parsed = self.retained_case(f'retained-{index:02d}-injected', package, mode, state, policy, target['ordinal'])
            report = self.retained_oracle.injected(self.oracle, actual, result.stdout, result.stderr, parsed, target)
            record.update(injected=actual, report=report, passed=True)
            self.save()
        controls = [('noop-sync', noop)] + ([('historical', historical)] if historical else [])
        for name, library in controls:
            for index, (mode, state, policy) in enumerate(self.retained_oracle.NEGATIVES):
                result, row, parsed = self.retained_case(f'retained-{name}-{index}-calibration', library, mode, state, policy)
                target = self.retained_oracle.calibration(self.oracle, row, result.stdout, result.stderr, parsed, False)
                record = {'provider': name, 'backend': mode, 'state': state, 'policy': policy, 'calibration': row, 'target': target}
                self.receipt['retained_EIO_negatives'].append(record)
                self.save()
                result, actual, parsed = self.retained_case(f'retained-{name}-{index}-injected', library, mode, state, policy, target['ordinal'])
                self.retained_oracle.calibration(self.oracle, actual, result.stdout, result.stderr, parsed, False)
                require('(INJECTED)' not in Path(actual['path'], 'syscalls.trace').read_text(), 'no-op retained control failed by unrelated injection')
                record.update(injected=actual, passed=True,
                    rejection='successful retained-state creation; no force fence; return0 fails exact rc==EIO assertion before later business work')
                self.save()
        require(len(self.receipt['retained_EIO']) == 27 and all(row['passed'] for row in self.receipt['retained_EIO']), 'retained writer positive grid incomplete')
        require(len(self.receipt['retained_EIO_negatives']) == 3 * len(controls)
                and all(row['passed'] for row in self.receipt['retained_EIO_negatives']), 'retained no-op controls incomplete')

    def weak_copy(self, backend: str, package: Path) -> None:
        folder = self.out / 'runs' / ('weak-copy-' + backend)
        folder.mkdir(parents=True)
        database = folder / 'database'
        self.successful('weak-copy-init-' + backend, [self.helper, backend, database])
        self.check_mode(database, backend)
        app = self.apps['copies']
        env = self.app_env(package, '1.0.2')
        loader = self.command('weak-copy-loader-' + backend, [app['path']], dict(env, LD_TRACE_LOADED_OBJECTS='1'))
        require(loader.returncode == 0 and str(package) in loader.stdout.decode(), 'weak copy loader mismatch')
        command = [app['path'], str(database), str(folder / 'normal'), str(folder / 'compact'), str(folder / 'archive')]
        started = time.monotonic()
        failure, line, stdout = None, b'', b''
        with (folder / 'stderr').open('wb') as stderr:
            process = OWNED.start(command, env=dict(env, LD_DEBUG='libs,bindings'), stdin=subprocess.PIPE,
                                  stdout=subprocess.PIPE, stderr=stderr)
            try:
                line = read_ready(process, b'WEAK_SOURCE_READY\n', self.a.timeout)
                before = (database / 'CURRENT').read_bytes()
                require(len(before) == 176 and before[:8] == b'ULMDBWK1'
                        and int.from_bytes(before[12:16], 'little') & 2, 'copy source is not actual weak176 data-debt state')
                stdout, _ = process.communicate(input=b'GO\n', timeout=self.a.timeout)
            except BaseException as exc:
                failure = exc
                line = getattr(exc, 'observed_stdout', line)
                with OWNED.ignore_interrupts():
                    try:
                        stdout, _ = OWNED.stop(process)
                    except BaseException as cleanup:
                        stdout = getattr(cleanup, 'stdout', b'')
                        failure = cleanup
        (folder / 'stdout').write_bytes(line + stdout)
        self.sequence += 1
        command_row = {'label': 'weak-copy-' + backend, 'argv': command, 'pid': process.pid,
            'returncode': process.returncode, 'stdout': str((folder / 'stdout').relative_to(self.out)),
            'stderr': str((folder / 'stderr').relative_to(self.out)), 'stdout_sha256': digest(folder / 'stdout'),
            'stderr_sha256': digest(folder / 'stderr'), 'seconds_observed_not_performance': time.monotonic() - started,
            'environment': {k: v for k, v in dict(env, LD_DEBUG='libs,bindings').items() if k.startswith(('LD_', 'ULMDB_'))}}
        if failure:
            command_row['failure'] = {'type': type(failure).__name__, 'message': str(failure)}
        with (self.out / 'commands.jsonl').open('a') as stream:
            stream.write(json.dumps(command_row) + '\n')
        self.receipt['command_count'] = self.sequence
        self.artifact(folder / 'stdout')
        self.artifact(folder / 'stderr')
        self.save()
        OWNED.forget(process)
        if failure:
            raise failure
        require(process.returncode == 0 and b'weak-copy-and-old-borrow=ok' in stdout, 'weak copy/old borrowed bytes oracle failed')
        bound = self.bindings(app, package, (folder / 'stderr').read_bytes())
        require(digest(Path(app['path'])) == app['sha256'], 'copy executable drifted')
        shapes = {}
        for name, directory in [('source', database), ('normal', folder / 'normal'), ('compact', folder / 'compact'), ('archive-restored', folder / 'archive.ulmdb')]:
            current = (directory / 'CURRENT').read_bytes()
            shapes[name] = {'bytes': len(current), 'magic': current[:8].decode(), 'sha256': digest(directory / 'CURRENT')}
            if name == 'source':
                require(len(current) == 176 and current[:8] == b'ULMDBWK1', 'source weak state silently rewritten')
            else:
                require((len(current) == 64 and current[:8] in {b'ULMDBCUR', b'ULMDBC03', b'ULMDBT04'})
                        or (len(current) == 144 and current[:8] == b'ULMDBPUB'), 'export is not a strong supported selector')
        require((folder / 'archive').read_bytes()[:8] == b'ULMDBCP\0', 'copyfd archive signature changed')
        self.receipt['extra_controls'].append({'kind': 'weak-copy', 'backend': backend, 'passed': True,
             'provider_sha256': digest(package), 'executable_sha256': app['sha256'], 'actual_bindings': bound,
             'actual_weak_source_before_copy_sha256': hashlib.sha256(before).hexdigest(), 'selectors': shapes,
             'archive_sha256': digest(folder / 'archive'),
             'scope': 'actual weak-source exports/held old borrow/private-write exclusion; not reference archive-layout parity'})
        self.save()

    def archive_copy_contract(self, backend: str, package: Path) -> None:
        """Generate and admit archives solely through the public C copy APIs."""
        folder = self.out / 'runs' / ('archive-contract-' + backend)
        folder.mkdir(parents=True, exist_ok=False)
        source = folder / 'source'
        self.successful('archive-contract-init-' + backend, [self.helper, backend, source])
        self.check_mode(source, backend)
        app = self.apps['archives']
        env = self.app_env(package, '1.0.2')
        loader = self.command('archive-contract-loader-' + backend, [app['path']], dict(env, LD_TRACE_LOADED_OBJECTS='1'))
        require(loader.returncode == 0 and str(package) in loader.stdout.decode(), 'archive contract loader mismatch')
        result = self.command('archive-contract-' + backend, [app['path'], folder], dict(env, LD_DEBUG='libs,bindings'))
        require(result.returncode == 0 and result.stdout == b'copy-archives-ok rejected=34\n',
                'public archive raw/compact history, mutable independence, FD errors or strict rejection contract failed')
        bound = self.bindings(app, package, result.stderr)
        require(digest(Path(app['path'])) == app['sha256'], 'archive contract executable drifted')
        archives = {}
        for name in ['raw', 'compact']:
            archive = folder / (name + '.archive')
            with archive.open('rb') as stream:
                require(stream.read(12) == b'ULMDBCP\0\x02\0\0\0', 'public fd copy did not emit archivev2')
            restored = folder / (name + '.archive.ulmdb')
            archives[name] = {'sha256': digest(archive), 'selected_magic': self.check_mode(restored, backend)}
        require(len(list(folder.glob('bad-*.archive'))) == 34, 'malformed archive corpus incomplete')
        require(not list(folder.glob('bad-*.archive.ulmdb')), 'malformed archive destination was published')
        for directory in [folder, folder / 'tmp']:
            require(not list(directory.glob('.ulmdb-copy-*')), 'owned archive staging leaked')
        for path in sorted(folder.rglob('*')):
            if path.is_file():
                self.artifact(path)
        self.receipt['extra_controls'].append({'kind': 'public-archive-contract', 'backend': backend, 'passed': True,
            'provider_sha256': digest(package), 'executable_sha256': app['sha256'], 'actual_bindings': bound,
            'malformed_archives_rejected': 34, 'archives': archives, 'source_selected_magic': self.check_mode(source, backend),
            'scope': 'public copyfd/copyfd2 v2 emission, raw D2/P1 and genuine compact1/0, retained borrowed A, excluded private writer, mutable copy/source independence, FD errors and nonpublication; adapter archive only'})
        self.save()

    def perform(self) -> None:
        self.source_setup()
        self.helper_provenance()
        self.references()
        package = self.provider('package', library_at(self.a.package))
        self.inventory_setup(package)
        self.smoke('package', package)
        self.corpus(package)
        self.backend_samples(package)
        archive = self.out / 'inputs/archive.a'
        self.copy(self.a.archive, archive)
        witness = self.relink('archive-witness', archive)
        self.smoke('archive-witness', witness)
        self.sample_provider('archive-witness', witness)
        noop = self.relink('noop-sync', archive, noop=True)
        self.smoke('noop-sync', noop)
        self.sample_provider('noop-sync', noop, deficient=True)
        old = None
        if self.a.negative_package is not None:
            old = self.provider('historical', library_at(self.a.negative_package))
            require(self.common56 <= self.exports('historical', old), 'historical provider lost unrelated common functions')
            self.smoke('historical', old)
            self.sample_provider('historical', old, historical=True)
        self.observer_controls()
        self.eio_controls(package, noop)
        self.retained_controls(package, noop, old)
        self.mapped_controls(package, archive, noop)
        self.mapped_retry_controls(package)
        self.extra_controls(package)
        require(len(self.receipt['cases']) == 640 and len(self.receipt['backend_samples']) == 27, 'positive evidence is incomplete')
        require(len([r for r in self.receipt['extra_controls'] if r['kind'] == 'creator']) == 8, 'creator controls incomplete')
        require(len([r for r in self.receipt['extra_controls'] if r['kind'] == 'weak-copy']) == 3, 'weak export controls incomplete')
        require(len([r for r in self.receipt['extra_controls'] if r['kind'] == 'public-archive-contract']) == 3,
                'public archive emission/admission controls incomplete')
        require(len(self.receipt['mapped_controls']) == 4 and len(self.receipt['mapped_backend_samples']) == 2
                and len(self.receipt['mapped_retry']) == 24, 'mapped mandatory controls incomplete')
        for path, expected in self.protected.items():
            require(digest(Path(path)) == expected, 'source/input drift: ' + path)
        for path, expected in self.generated.items():
            require(digest(self.out / path) == expected, 'generated executable/source/provider/evidence drift: ' + path)
        after = {str(path.relative_to(HERE)): digest(path) for path in HERE.rglob('*')
            if path.is_file() and '__pycache__' not in path.parts and path.suffix in {'.py', '.c', '.rs', '.json', '.md'}}
        require(after == self.source_snapshot, 'qualification source inventory changed during run')
        self.receipt['qualification_source_after'] = after
        require(not OWNED.ACTIVE, 'owned process registry is not empty at successful completion')
        self.receipt.update(complete=True, passed=True, protected_inputs=self.protected, generated_artifacts=self.generated,
            source_before_after_equal=True, primary_buffered_cases=326, backend_buffered_cases=27,
            archive_witness_buffered_cases=9, primary_mapped_cases=314, physical_WRITEMAP_unqualified_cases=0,
            mapped_acceptance={'status': 'passed-declared314-and-mandatory-controls', 'required_attempts': 314, 'passed_attempts': 314},
            mapped_backend_cases=2, mapped_observer_controls=16, mapped_deficient_provider_rejections=2,
            mapped_error_lifetime_reference_cases=12, mapped_error_lifetime_candidate_cases=12,
            mandatory_noop_provider_rejections=9, mandatory_observer_rejections=2,
            retained_writer_EIO_calibrations=27, retained_writer_EIO_injections=27,
            mandatory_retained_noop_rejections=3,
            effective_fences='all root fences retained; capacity-hint origin distinguished and first-seed incidental CURRENT strengthening explicit',
            strengthening='NoMeta selector inode fsync retained; data/final policies sampled separately and selected sync excludes unpublished prepared assets',
            claims_excluded=['mapped combinations outside declared314 (transforms/FIXEDMAP/nondefault geometry)', 'power-loss/performance acceptance',
                             'general NOLOCK/G19 support', 'unfinished G03/typed alias/G14 qualification',
                             'arbitrary helper source implies C qualification'])
        self.artifact(self.out / 'commands.jsonl')
        self.save()


def main() -> int:
    ap = argparse.ArgumentParser(description=__doc__)
    ap.add_argument('--package', type=Path, required=True, help='packaged directory or actual liblmdb.so.0')
    ap.add_argument('--archive', type=Path, required=True, help='matching normal Rust/package static archive; copied, never modified')
    ap.add_argument('--source-root', type=Path, help='repository pin manifests and frozen C fork/copy fixtures')
    ap.add_argument('--lmdb-source', type=Path, required=True, help='pinned1.0.2 source or checkout')
    ap.add_argument('--legacy-source', type=Path, required=True, help='pinned0.9.31 source or checkout')
    ap.add_argument('--current09-source', type=Path, required=True, help='pinned0.9.36 source or checkout')
    ap.add_argument('--backend-init', type=Path, required=True, help='explicit empty Native mode initializer')
    ap.add_argument('--backend-init-provenance', type=Path, required=True, help='complete source/build provenance from build_backend_init.py')
    ap.add_argument('--negative-package', type=Path, help='optional preserved historical flag-only provider; generated no-op control is always required')
    ap.add_argument('--output', type=Path, required=True, help='new directory; failed partial receipts are retained')
    ap.add_argument('--timeout', type=float, default=60, help='owned process deadline, never an accepted engine rejection')
    a = ap.parse_args()
    OWNED.install_handlers()
    a.source_root = (a.source_root or find_root()).resolve()
    a.archive = a.archive.resolve()
    require(a.timeout > 0, 'timeout must be positive')
    run = Qualification(a)
    try:
        run.perform()
    except BaseException as exc:
        with OWNED.ignore_interrupts():
            OWNED.cleanup_all(run.out)
            run.receipt.update(complete=False, passed=False, error={'type': type(exc).__name__, 'message': str(exc)},
                               protected_inputs=run.protected, generated_artifacts=run.generated)
            run.save()
            print(f'durability qualification failed: {exc}; retained receipt: {run.out / "receipt.json"}', file=sys.stderr)
        return 1
    print(run.out / 'receipt.json')
    return 0


if __name__ == '__main__':
    raise SystemExit(main())
