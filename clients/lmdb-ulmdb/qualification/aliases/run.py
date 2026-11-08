#!/usr/bin/env python3
"""Pinned public C alias/PREV conformance and separately classified safe boundaries.

Sources, cases, process ownership and pins are repository-owned. No runtime read
of a development receipt or target/qualification experiment is permitted.
"""
import argparse
from collections import Counter
import hashlib
import json
import os
from pathlib import Path
import re
import resource
import shutil
import subprocess
import sys
import traceback
import types

HERE = Path(__file__).resolve().parent
PREVIOUS = 33554432
READONLY = 131072
NOLOCK = 4194304
VERSIONS = ['0.9.31', '0.9.36', '1.0.2']
ACTOR_SOURCE = 'clients/lmdb-ulmdb/tests/parity_aliases/m3-previous-reference-expanded/'
QUALIFIED_COPY_ROLES = {
    'seeds': [1, 3],
    'roles': ['readonly-PREV', 'writable-PREV-before', 'writable-PREV-after-C1', 'normal-after-readonly-setter'],
    'nolock': [False, True], 'compact': [False, True],
    'raw_invalid_reference_outputs': 12,
}


def sha(path):
    return hashlib.sha256(Path(path).read_bytes()).hexdigest()


def checked_tooling():
    manifest = json.loads((HERE / 'SOURCES.json').read_text())
    expected = manifest['owned_tooling_sha256']
    actual = {p.name for p in HERE.iterdir() if p.is_file() and p.name != 'SOURCES.json'}
    if actual != set(expected):
        raise ValueError('owned tooling inventory differs from manifest')
    for name, digest in expected.items():
        path = HERE / name
        if Path(name).name != name or path.is_symlink() or not path.is_file() or sha(path) != digest:
            raise ValueError('owned tooling hash/type drift: ' + name)
    return manifest


def load_owned_modules():
    # Compile checked bytes directly: -B alone still permits reading stale pyc.
    # No owned import executes before this source/hash admission boundary.
    manifest = checked_tooling()
    loaded = {}
    for name in ['processes', 'commands', 'actors']:
        path = HERE / (name + '.py')
        source = path.read_bytes()
        if hashlib.sha256(source).hexdigest() != manifest['owned_tooling_sha256'][path.name]:
            raise ValueError('tool changed during module admission')
        module = types.ModuleType(name)
        module.__file__ = str(path)
        module.__package__ = ''
        sys.modules[name] = module
        exec(compile(source, str(path), 'exec'), module.__dict__)
        loaded[name] = module
    globals().update(processes=loaded['processes'], commands=loaded['commands'], Agent=loaded['actors'].Agent)


def clean_env(provider, version):
    env = dict(os.environ)
    for key in ['LD_PRELOAD', 'LD_AUDIT', 'LD_DEBUG', 'LD_DEBUG_OUTPUT',
                'LD_TRACE_LOADED_OBJECTS', 'LD_LIBRARY_PATH', 'ULMDB_LIBRARY',
                'ULMDB_MIGRATOR', 'ULMDB_LMDB_ABI']:
        env.pop(key, None)
    env.update(LD_LIBRARY_PATH=str(provider.parent), LD_BIND_NOW='1', LC_ALL='C',
               ULMDB_LMDB_ABI='1.0' if version == '1.0.2' else version)
    return env


def business(directory):
    return {str(p.relative_to(directory)): sha(p) for p in sorted(directory.rglob('*'))
            if p.is_file() and (p.name in ['data.mdb', 'CURRENT', 'PREVIOUS']
             or p.relative_to(directory).parts[0] in ['segments', 'commits', 'cohorts'])}


class Run:
    def __init__(self, args):
        self.args = args
        self.root = (args.source_root or HERE.parents[3]).resolve()
        self.cases = json.loads((HERE / 'cases.json').read_text())
        declaration = self.cases['modern_copy_roles']
        if declaration != QUALIFIED_COPY_ROLES or any(type(x) is not bool for x in declaration['nolock'] + declaration['compact']) or any(type(x) is not int for x in declaration['seeds']) or type(declaration['raw_invalid_reference_outputs']) is not int:
            raise ValueError('unqualified copy-role declaration; add reference evidence and controller support explicitly')
        if Counter(c['classification'] for c in self.cases['modern_original']) != {
            'planned_unchanged_executable_reference_replay': 63,
            'copy_history_dependency_not_claimed': 6,
            'preserved_upstream_assertion_safe_explicit_boundary': 15,
        }:
            raise ValueError('modern original case inventory drift')
        self.groups = set(args.groups.split(','))
        if not self.groups <= {'aliases', 'previous', 'copy', 'boundaries', 'legacy'}:
            raise ValueError('unknown group')
        self.output = args.output.resolve()
        self.output.mkdir(parents=True, exist_ok=False)
        self.receipt = {'schema': 1, 'scope': 'Pinned public C aliases/PREV plus explicit safe boundaries',
            'complete': False, 'passed': False, 'groups': sorted(self.groups), 'commands': [],
            'references': [], 'cases': [], 'claim_boundaries': [
                'PREVSNAPSHOT positive differential is1.0.2 only; both0.9 headers lack it',
                '15 upstream assertion cases are adapter safe-history controls, never positive crash matches',
                '12 raw older-view corrupt-output cases require adapterENOTSUP, never corrupt-output emulation',
                'no powerloss, performance, Range, Resident or other-platform qualification']}
        self.apps = {}
        self.providers = {}
        self.source_rows = {}
        self.reference_results = {}
        self.executable_hashes = {}
        self.save()

    def save(self):
        (self.output / 'receipt.json').write_text(json.dumps(self.receipt, indent=2) + '\n')

    def command(self, name, argv, env=None, timeout=20, expected=0):
        return commands.run(self, name, argv, env=env, timeout=timeout, expected=expected)

    def sources(self):
        manifest = checked_tooling()
        self.fixture_root = self.root / 'clients/lmdb-ulmdb/tests/parity_aliases'
        self.fixture_inventory = {str(p.relative_to(self.root)) for p in self.fixture_root.rglob('*') if p.is_file()}
        if self.fixture_inventory != set(manifest['all_sources_sha256']):
            raise ValueError('fixture inventory differs from source manifest')
        self.tool_inventory = {p.name for p in HERE.iterdir() if p.is_file()}
        for relative, expected in manifest['all_sources_sha256'].items():
            path = (self.root / relative).resolve()
            if not path.is_relative_to(self.root) or sha(path) != expected:
                raise ValueError('source drift: ' + relative)
            self.source_rows[relative] = expected
        # Include runner/case/orchestration/process helper bytes, independently
        # of historical origin paths in the provenance manifest.
        for path in sorted(HERE.glob('*')):
            if path.is_file():
                self.source_rows[str(path.relative_to(self.root))] = sha(path)
        self.receipt['source_inputs'] = self.source_rows
        if self.args.reference_only:
            return
        original = (self.args.provider or self.args.package / 'lib/liblmdb.so.0').resolve()
        expected = self.args.provider_sha256 or sha(original)
        if sha(original) != expected:
            raise ValueError('provider hash mismatch')
        folder = self.output / 'provider'
        folder.mkdir()
        selected = folder / 'liblmdb.so.0'
        shutil.copyfile(original, selected)
        selected.chmod(0o444)
        if sha(original) != expected or sha(selected) != expected:
            raise ValueError('provider changed during freeze')
        self.candidate = selected
        self.receipt['provider'] = {'original': str(original), 'sha256': expected}

    def references(self):
        pins = json.loads((HERE / 'reference_pins.json').read_text())['references']
        paths = {'0.9.31': self.args.legacy_source or Path.home() / '.cache/ulmdb-compat/lmdb-0.9.31',
                 '0.9.36': self.args.current09_source or Path.home() / '.cache/ulmdb-compat/lmdb-0.9.36',
                 '1.0.2': self.args.lmdb_source or self.root / 'benchmark/support/lmdb/vendor'}
        for version in VERSIONS:
            folder = self.output / 'references' / version
            source = folder / 'source'
            source.mkdir(parents=True)
            for entry in pins[version]['files']:
                original = paths[version] / entry['path']
                if original.is_symlink() or original.stat().st_size != entry['bytes'] or sha(original) != entry['sha256']:
                    raise ValueError('pinned reference input mismatch: ' + str(original))
                shutil.copyfile(original, source / entry['path'])
            provider = folder / 'liblmdb.so.0'
            self.command('reference-' + version, ['cc', '-std=c11', '-O2', '-fPIC', '-shared', '-pthread',
                '-D_GNU_SOURCE', '-Wl,-soname,liblmdb.so.0', '-I', source, source / 'mdb.c', source / 'midl.c', '-o', provider], timeout=120)
            (folder / 'liblmdb.so').symlink_to('liblmdb.so.0')
            self.providers[version] = provider
            self.receipt['references'].append({'version': version, 'commit': pins[version]['commit'],
                'provider_sha256': sha(provider), 'pin_sha256': {e['path']: e['sha256'] for e in pins[version]['files']}})
        self.save()

    def app(self, source, version):
        key = source, version
        if key in self.apps:
            return self.apps[key]
        provider = self.providers[version]
        folder = self.output / 'executables' / version / hashlib.sha256(source.encode()).hexdigest()[:12]
        folder.mkdir(parents=True)
        executable = folder / 'probe'
        self.command('compile-' + Path(source).stem + '-' + version,
            ['cc', '-std=c11', '-O2', '-Wall', '-Wextra', '-Werror', '-pthread', '-I', provider.parent / 'source',
             self.root / source, '-L', provider.parent, '-llmdb', '-ldl', '-o', executable], timeout=60)
        dynamic = self.command('dynamic', ['readelf', '-d', executable])[0].decode()
        if '[liblmdb.so.0]' not in dynamic or re.search(r'\(NEEDED\).*\[/', dynamic):
            raise RuntimeError('executable does not select basenameSONAME0')
        imports = self.command('imports', ['nm', '-D', '--undefined-only', executable])[0].decode()
        names = sorted(set(re.findall(r'\b(mdb_\w+)\b', imports)))
        self.apps[key] = executable, names
        self.executable_hashes[str(executable)] = sha(executable)
        self.receipt.setdefault('executables', []).append({'source': source, 'version': version,
            'path': str(executable), 'sha256': sha(executable), 'imports': names})
        self.save()
        return self.apps[key]

    def provider_proof(self, app, names, provider, version, folder, actor=False):
        env = clean_env(provider, version)
        out, _, _ = self.command('loader', [app], env | {'LD_TRACE_LOADED_OBJECTS': '1'})
        (folder / 'loader.stdout').write_bytes(out)
        if str(provider).encode() not in out:
            raise RuntimeError('wrong runtime provider')
        # BIND_NOW resolves every import even in the caps/usage path, with no DBI
        # or transaction crossing between dynamic-library instances.
        cmd = [app, 'caps'] if actor else [app]
        _, err, _ = self.command('bindings', cmd, env | {'LD_DEBUG': 'libs,bindings'}, expected=0 if actor else None)
        bound = [n for n in names if re.search(r'binding file ' + re.escape(str(app)) + r'.* to ' +
            re.escape(str(provider)) + r'.*symbol [\x27`]' + re.escape(n) + r"['`]", err.decode(errors='replace'))]
        if bound != names:
            raise RuntimeError('actual import-provider mismatch')
        return env, bound

    def arms(self, version):
        yield 'reference', self.providers[version]
        if not self.args.reference_only:
            yield 'adapter', self.candidate

    def leaf_cases(self):
        for case in self.cases['leaf_cases']:
            selected_group = 'copy' if case['group'].startswith('copy-') else 'aliases'
            if selected_group not in self.groups:
                continue
            for version in case['versions']:
                app, names = self.app(case['source'], version)
                expected = None
                for arm, provider in self.arms(version):
                    folder = self.output / 'leaf' / case['id'] / version / arm
                    folder.mkdir(parents=True)
                    env, bound = self.provider_proof(app, names, provider, version, folder)
                    argv = [app, *[a.replace('{database}', str(folder / 'database')) for a in case['args']]]
                    out, err, _ = self.command('leaf', argv, env, timeout=20)
                    (folder / 'stdout').write_bytes(out)
                    (folder / 'stderr').write_bytes(err)
                    semantic = out
                    if case.get('provider_stdout'):
                        prefix = ('PROVIDER ' + str(provider) + '\n').encode()
                        if not out.startswith(prefix):
                            raise RuntimeError('actual dladdr provider prefix mismatch')
                        semantic = out[len(prefix):]
                    if arm == 'reference':
                        expected = semantic
                    passed = arm == 'reference' or semantic == expected
                    self.receipt['cases'].append({'id': case['id'], 'version': version, 'arm': arm,
                        'classification': 'positive-reference-trace', 'journeys': case['journeys'],
                        'executable_sha256': sha(app), 'provider_sha256': sha(provider), 'bindings': bound,
                        'stdout_sha256': sha(folder / 'stdout'),
                        'semantic_sha256': hashlib.sha256(semantic).hexdigest(), 'passed': passed})
                    self.save()
                    if not passed:
                        raise RuntimeError('reference mismatch: ' + case['id'] + '/' + version)

    def actor_case(self, group, name, count, body, arm, app, provider,
                   classification='positive-reference-trace', compare=True):
        folder = self.output / 'modern' / group / name / arm
        folder.mkdir(parents=True)
        database = folder / 'database'
        row = {'id': group + '/' + name, 'arm': arm, 'version': '1.0.2', 'directory': str(folder),
               'agents': [], 'classification': classification, 'complete': False, 'passed': False,
               'executable_sha256': sha(app), 'provider_sha256': sha(provider)}
        self.receipt['cases'].append(row)
        env = clean_env(provider, '1.0.2')
        self.command('seed', [app, 'seed', database, count], env)
        live = []
        def spawn(label, flags=0, path=database, allow_error=False, wait=True):
            actor = Agent(label, path, flags, row, app, env)
            live.append(actor)
            if wait:
                actor.opened(allow_error)
            return actor
        errors = []
        try:
            body(database, spawn, row)
        except BaseException as error:
            errors.append({'phase': 'body', 'type': type(error).__name__, 'message': str(error)})
        finally:
            for actor in reversed(live):
                if not actor.output.closed:
                    try:
                        actor.close()
                    except BaseException as error:
                        errors.append({'phase': 'cleanup', 'actor': actor.label, 'type': type(error).__name__, 'message': str(error)})
        if errors:
            row.update(complete=True, errors=errors)
            self.save()
            raise RuntimeError('actor case failed: ' + str(errors))
        outputs = {a['label']: {'stdout': Path(a['stdout']).read_text(), 'exit': a['exit_code']}
                   for a in row['agents']}
        row.update(complete=True, passed=True, semantic_trace_sha256=hashlib.sha256(json.dumps(outputs, sort_keys=True).encode()).hexdigest())
        key = group, name
        if not compare:
            pass
        elif arm == 'reference':
            self.reference_results[key] = outputs
        else:
            row['passed'] = outputs == self.reference_results[key]
        self.save()
        if not row['passed']:
            raise RuntimeError('modern semantic mismatch: ' + row['id'])

    def modern(self):
        kinds = set()
        if 'previous' in self.groups:
            kinds.add('planned_unchanged_executable_reference_replay')
        if 'copy' in self.groups:
            kinds.add('copy_history_dependency_not_claimed')
        for group in ['base', 'followup', 'active']:
            selected = {c['name'] for c in self.cases['modern_original'] if c['group'] == group and c['classification'] in kinds}
            if not selected:
                continue
            source = ACTOR_SOURCE + ('previous_active_probe.c' if group == 'active' else 'previous_probe.c')
            app, names = self.app(source, '1.0.2')
            for arm, provider in self.arms('1.0.2'):
                folder = self.output / 'actor-providers' / group / arm
                folder.mkdir(parents=True)
                self.provider_proof(app, names, provider, '1.0.2', folder, actor=True)
                seen = set()
                def case(name, count, body):
                    if name in selected:
                        self.actor_case(group, name, count, body, arm, app, provider)
                        seen.add(name)
                context = {'Path': Path, 'prev': PREVIOUS, 'ro': READONLY, 'nolock': NOLOCK, 'case': case}
                path = HERE / (group + '_cases.py')
                exec(compile(path.read_text(), str(path), 'exec'), context)
                if seen != selected:
                    raise RuntimeError('case omission: ' + str(selected - seen))

    def legacy(self):
        source = 'clients/lmdb-ulmdb/tests/parity_aliases/m3-c-logical-alias-work/legacy-prev-boundary/legacy_prev_boundary.c'
        for version in VERSIONS[:2]:
            app, names = self.app(source, version)
            header = self.providers[version].parent / 'source/lmdb.h'
            implementation = self.providers[version].parent / 'source/mdb.c'
            if b'PREVSNAPSHOT' in header.read_bytes() or b'PREVSNAPSHOT' in implementation.read_bytes():
                raise RuntimeError('legacy capability inventory drift')
            for arm, provider in self.arms(version):
                folder = self.output / 'legacy' / version / arm
                folder.mkdir(parents=True)
                env, bindings = self.provider_proof(app, names, provider, version, folder)
                database = folder / 'database'
                for mode, expected in [('seed', b'header.previous=0\nseed=A\n'),
                        ('probe', b'header.previous=0\nunknown.previous.open=22\n'),
                        ('check', b'header.previous=0\nchecked.id=1 value=A\n')]:
                    before = business(database) if mode != 'seed' else None
                    out, _, _ = self.command('legacy-' + mode, [app, mode, database], env)
                    if out != expected or before is not None and business(database) != before:
                        raise RuntimeError('legacy unknown flag boundary mismatch')
                self.receipt['cases'].append({'id': 'legacy-unknown-prev', 'version': version, 'arm': arm,
                    'classification': 'header-absence-and-unknown-flag-rejection', 'bindings': bindings, 'passed': True})
                self.save()

    def safe_history(self):
        # Never execute the15 known upstream assertion cases. Exercise the
        # adapter's exact ENOTSUP and unchanged business roots instead.
        if self.args.reference_only:
            return
        app, names = self.app(ACTOR_SOURCE + 'previous_probe.c', '1.0.2')
        folder = self.output / 'actor-providers/safe-history/adapter'
        folder.mkdir(parents=True)
        self.provider_proof(app, names, self.candidate, '1.0.2', folder, actor=True)
        for case in self.cases['modern_original']:
            if case['classification'] != 'preserved_upstream_assertion_safe_explicit_boundary':
                continue
            name, group = case['name'], case['group']
            count = int(re.search(r'seed(\d+)', name)[1])
            flags = PREVIOUS
            if 'readonly1' in name:
                flags |= READONLY
            if 'nolock1' in name:
                flags |= NOLOCK
            def body(db, spawn, row, flags=flags):
                owner = spawn('rollback-owner')
                owner.send('write-rollback undone first')
                owner.send('read restored')
                owner.assert_view('restored', count, f'seed{count}')
                owner.close()
                before = business(db)
                previous = spawn('unavailable-history', flags, allow_error=True)
                if previous.ready or previous.row['open_result'] != 95:
                    raise RuntimeError('unavailable history did not reject ENOTSUP')
                previous.close()
                if business(db) != before:
                    raise RuntimeError('unsupported history changed business files')
                owner = spawn('fresh-progress')
                owner.send('abort discarded after-rejection')
                owner.assert_abort(count + 1, f'seed{count}')
                owner.send('read restored-after-rejection')
                owner.assert_view('restored-after-rejection', count, f'seed{count}')
                owner.close()
                if business(db) != before:
                    raise RuntimeError('aborted progress changed business files')
            # actor_case normally compares the reference; this boundary is an
            # explicit one-arm safety policy, and is recorded separately.
            self.actor_case('safe-' + group, name, count, body, 'adapter', app, self.candidate,
                            classification='safe-unavailable-history-not-parity', compare=False)

    def copy_roles(self):
        app, names = self.app(ACTOR_SOURCE + 'previous_probe.c', '1.0.2')
        for arm, provider in self.arms('1.0.2'):
            folder = self.output / 'actor-providers/copy-roles' / arm
            folder.mkdir(parents=True)
            self.provider_proof(app, names, provider, '1.0.2', folder, actor=True)
        completed = 0
        matrix = self.cases['modern_copy_roles']
        for count in matrix['seeds']:
            for role in matrix['roles']:
                for nolock in matrix['nolock']:
                    for compact in matrix['compact']:
                        name = f'seed{count}-{role}-nolock{int(nolock)}-compact{int(compact)}'
                        # These12 exact reference paths write truncated/corrupt
                        # outputs. They are explicit safe-policy controls,
                        # never positive equality with a failed reference.
                        bug = (not compact and role in ['readonly-PREV', 'writable-PREV-before']) or (
                            not compact and not nolock and role in ['writable-PREV-after-C1', 'normal-after-readonly-setter'])
                        for arm, provider in self.arms('1.0.2'):
                            def body(db, spawn, row, arm=arm, compact=compact, nolock=nolock, role=role, bug=bug, count=count):
                                lockflag = NOLOCK if nolock else 0
                                if role == 'normal-after-readonly-setter':
                                    source = spawn('copy-owner', lockflag)
                                    setter = spawn('readonly-setter', PREVIOUS | READONLY | lockflag)
                                    setter.send('read setter-view')
                                else:
                                    source = spawn('copy-owner', PREVIOUS | lockflag | (READONLY if role == 'readonly-PREV' else 0))
                                if role == 'writable-PREV-after-C1':
                                    source.send('write C1 first')
                                source.send('read source-before-copy')
                                if bug and arm == 'adapter':
                                    source.assert_view('source-before-copy', count - 1,
                                                       f'seed{count - 1}' if count > 1 else None)
                                destination = Path(row['directory']) / 'backup'
                                before = business(db)
                                if bug and arm == 'adapter':
                                    source.send_copy_unsupported(destination)
                                    if not destination.is_dir() or list(destination.iterdir()):
                                        raise RuntimeError('unsupported raw copy created output data')
                                    if business(db) != before:
                                        raise RuntimeError('unsupported copy changed source')
                                    progress = spawn('fresh-progress')
                                    progress.send('abort discarded after-copy-rejection')
                                    read_id, value, first = count, f'seed{count}', None
                                    if role == 'normal-after-readonly-setter':
                                        read_id, value = count - 1, f'seed{count - 1}' if count > 1 else None
                                    elif role == 'writable-PREV-after-C1':
                                        value, first = 'C1', 'C1'
                                    progress.assert_abort(read_id + 1, value, first)
                                    progress.send('read source-after-rejection')
                                    progress.assert_view('source-after-rejection', read_id, value, first)
                                    progress.close()
                                    if business(db) != before:
                                        raise RuntimeError('aborted progress changed source')
                                    return
                                source.send(f'copy {destination} {int(compact)}')
                                source.close()
                                target = spawn('normal-backup', READONLY, path=destination, allow_error=bug)
                                if bug:
                                    expected = -30796 if count == 1 else -30797
                                    if target.ready or target.row['open_result'] != expected:
                                        raise RuntimeError('upstream raw-copy boundary drift')
                                    target.close()
                                    return
                                target.send('read backup-current')
                                target.close()
                                previous = spawn('previous-backup', PREVIOUS | READONLY, path=destination, allow_error=True)
                                if previous.ready:
                                    previous.send('read backup-previous')
                            classification = ('upstream-corrupt-copy-output' if arm == 'reference' else 'safe-raw-copy-rejection-not-parity') if bug else 'positive-reference-trace'
                            self.actor_case('copy-roles', name, count, body, arm, app, provider,
                                            classification=classification, compare=not bug)
                        completed += 1
        if completed != 32:
            raise RuntimeError('copy-role inventory incomplete')

    def finish(self):
        declared = self.expected_case_keys()
        expected = len(declared)
        if not expected or len(self.receipt['cases']) != expected:
            raise RuntimeError('executed case inventory mismatch: ' + str((len(self.receipt['cases']), expected)))
        actual = {}
        for row in self.receipt['cases']:
            key = row['id'], row['version'], row['arm']
            if key in actual:
                raise RuntimeError('duplicate executed case key: ' + str(key))
            actual[key] = row['classification']
        if actual != declared:
            raise RuntimeError('executed case keys/classifications differ from accepted declarations')
        if {str(p.relative_to(self.root)) for p in self.fixture_root.rglob('*') if p.is_file()} != self.fixture_inventory:
            raise RuntimeError('fixture files added/removed during run')
        if {p.name for p in HERE.iterdir() if p.is_file()} != self.tool_inventory:
            raise RuntimeError('qualification files added/removed during run')
        for path, expected in self.executable_hashes.items():
            if sha(path) != expected:
                raise RuntimeError('executed app changed during run')
        if not self.args.reference_only:
            if sha(self.receipt['provider']['original']) != self.receipt['provider']['sha256'] or sha(self.candidate) != self.receipt['provider']['sha256']:
                raise RuntimeError('provider changed during qualification')
        for relative, expected in self.source_rows.items():
            if sha(self.root / relative) != expected:
                raise RuntimeError('source changed during run: ' + relative)
        self.receipt.update(complete=True, passed=all(c['passed'] for c in self.receipt['cases']))
        self.save()

    def expected_case_keys(self):
        expected = {}
        arms = ['reference'] if self.args.reference_only else ['reference', 'adapter']
        def add(key, classification):
            if key in expected:
                raise ValueError('duplicate declared case key: ' + str(key))
            expected[key] = classification
        for case in self.cases['leaf_cases']:
            if ('copy' if case['group'].startswith('copy-') else 'aliases') in self.groups:
                for version in case['versions']:
                    for arm in arms:
                        add((case['id'], version, arm), 'positive-reference-trace')
        for case in self.cases['modern_original']:
            status = case['classification']
            if status == 'planned_unchanged_executable_reference_replay' and 'previous' in self.groups or status == 'copy_history_dependency_not_claimed' and 'copy' in self.groups:
                for arm in arms:
                    add((case['group'] + '/' + case['name'], '1.0.2', arm), 'positive-reference-trace')
            elif status == 'preserved_upstream_assertion_safe_explicit_boundary' and 'boundaries' in self.groups and not self.args.reference_only:
                add(('safe-' + case['group'] + '/' + case['name'], '1.0.2', 'adapter'), 'safe-unavailable-history-not-parity')
        if 'legacy' in self.groups:
            for version in VERSIONS[:2]:
                for arm in arms:
                    add(('legacy-unknown-prev', version, arm), 'header-absence-and-unknown-flag-rejection')
        if 'copy' in self.groups:
            matrix = self.cases['modern_copy_roles']
            for count in matrix['seeds']:
                for role in matrix['roles']:
                    for nolock in matrix['nolock']:
                        for compact in matrix['compact']:
                            name = f'seed{count}-{role}-nolock{int(nolock)}-compact{int(compact)}'
                            bug = not compact and (role in ['readonly-PREV', 'writable-PREV-before'] or not nolock)
                            for arm in arms:
                                classification = ('upstream-corrupt-copy-output' if arm == 'reference' else 'safe-raw-copy-rejection-not-parity') if bug else 'positive-reference-trace'
                                add(('copy-roles/' + name, '1.0.2', arm), classification)
        return expected


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument('--source-root', type=Path)
    selected = parser.add_mutually_exclusive_group()
    selected.add_argument('--package', type=Path)
    selected.add_argument('--provider', type=Path)
    parser.add_argument('--provider-sha256')
    parser.add_argument('--output', type=Path)
    parser.add_argument('--legacy-source', type=Path)
    parser.add_argument('--current09-source', type=Path)
    parser.add_argument('--lmdb-source', type=Path)
    parser.add_argument('--reference-only', action='store_true')
    parser.add_argument('--groups', default='aliases,previous,copy,boundaries,legacy')
    parser.add_argument('--list', action='store_true')
    args = parser.parse_args()
    load_owned_modules()
    if args.list:
        cases = json.loads((HERE / 'cases.json').read_text())
        print(json.dumps({'leaf_executions': sum(len(c['versions']) for c in cases['leaf_cases']),
              'leaf_journeys': sum(len(c['versions']) * c['journeys'] for c in cases['leaf_cases']),
              'modern': cases['classifications'], 'copy_roles': cases['modern_copy_roles']}, indent=2))
        return 0
    if not args.output or not args.reference_only and not (args.package or args.provider):
        parser.error('--output and --package/--provider are required (provider optional for --reference-only)')
    processes.install_handlers()
    resource.setrlimit(resource.RLIMIT_CORE, (0, 0))
    run = Run(args)
    try:
        run.sources()
        run.references()
        run.leaf_cases()
        run.modern()
        if 'legacy' in run.groups:
            run.legacy()
        if 'boundaries' in run.groups:
            run.safe_history()
        if 'copy' in run.groups:
            run.copy_roles()
        run.finish()
        return 0
    except BaseException as error:
        run.receipt.update(error={'type': type(error).__name__, 'message': str(error)}, traceback=traceback.format_exc())
        run.save()
        raise
    finally:
        processes.cleanup_all(run.output)


if __name__ == '__main__':
    sys.exit(main())
