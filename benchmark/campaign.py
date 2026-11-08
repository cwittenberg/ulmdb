#!/usr/bin/env python3
"""Run private, interleaved before/after benchmarks with preserved executables.

Example (candidate binaries must already have been built):
  python3 benchmark/campaign.py benchmark/work/baseline \
    --candidate-full /path/to/ulmdb-engine-benchmark \
    --candidate-millions /path/to/ulmdb-millions --output benchmark/work/iteration-1

No builds or public report writes occur. Baseline/candidate full-suite processes
alternate within each filesystem/round. Million ULMDB processes are paired;
contemporaneous LMDB/SQLite control processes run once each per rotated round
and are explicitly shared by the two captures. No failures/outliers are retried.
"""
from __future__ import annotations

import argparse
import copy
import ctypes
import datetime
import importlib.util
import json
import os
from pathlib import Path
import re
import shutil
import subprocess
import sys
import tempfile
import time

HERE = Path(__file__).resolve().parent


def module(name, path):
    spec = importlib.util.spec_from_file_location(name, path)
    loaded = importlib.util.module_from_spec(spec)
    spec.loader.exec_module(loaded)
    return loaded


runner = module('ulmdb_public_benchmark_runner', HERE / 'run.py')
comparison = module('ulmdb_benchmark_comparison', HERE / 'compare.py')
require = comparison.require


SQLITE_VERSION_KEYS = {'sqlite', 'source_id', 'link_library_path', 'link_library_sha256'}


def sqlite_reference_context(initial, accepted, runtime):
    """Only a historical file hash may differ; current authority is accepted."""
    for versions in [initial, accepted, runtime]:
        require(set(versions) == SQLITE_VERSION_KEYS
                and all(isinstance(value, str) and value for value in versions.values()),
                'SQLite version/link identity is incomplete')
        require(re.fullmatch('[0-9a-f]{64}', versions['link_library_sha256']) is not None,
                'SQLite library digest is invalid')
    stable = SQLITE_VERSION_KEYS - {'link_library_sha256'}
    require(all(initial[key] == accepted[key] for key in stable),
            'initial/accepted SQLite version, source_id or link path differs')
    require(runtime == accepted, 'current SQLite runtime identity differs from accepted reference')
    return {'archived_initial': copy.deepcopy(initial), 'archived_accepted': copy.deepcopy(accepted),
            'runtime': copy.deepcopy(runtime),
            'historical_link_hash_only_change': initial != accepted,
            'policy': 'historical hash context only; all current processes use accepted exact DSO; '
                      'archived observations are unchanged and controls are measured contemporaneously'}


def sqlite_runtime_versions(path):
    """Metadata calls only: no database, query, fixture or timed work."""
    try:
        library = ctypes.CDLL(str(path))
        result = {}
        for key, symbol in [('sqlite', 'sqlite3_libversion'), ('source_id', 'sqlite3_sourceid')]:
            function = getattr(library, symbol)
            function.argtypes, function.restype = [], ctypes.c_char_p
            value = function()
            require(isinstance(value, bytes) and value, 'SQLite runtime metadata is empty')
            result[key] = value.decode('utf-8')
        return result
    except (OSError, AttributeError, UnicodeDecodeError) as error:
        raise ValueError(f'cannot inspect SQLite runtime metadata: {error}') from error


def verify_sqlite_runtime_pin(pin):
    versions = pin['versions']
    path = Path(versions['link_library_path'])
    require(path.is_absolute() and path.is_file() and path.resolve() == path,
            'pinned SQLite canonical path changed')
    require(runner.sha(path) == versions['link_library_sha256'], 'pinned SQLite runtime bytes changed')
    current = sqlite_runtime_versions(path)
    require(all(current[key] == versions[key] for key in ['sqlite', 'source_id']),
            'pinned SQLite runtime version/source_id changed')
    require(runner.sha(path) == versions['link_library_sha256'], 'pinned SQLite runtime bytes changed')


def inspect_sqlite_runtime(executable, expected, env=None):
    """Verify the actual ELF loader choice and its bytes, not merely a link hint."""
    environment = dict(os.environ if env is None else env)
    require(not environment.get('LD_PRELOAD') and not environment.get('LD_AUDIT'),
            'SQLite runtime pin refuses loader instrumentation')
    pin = {'versions': copy.deepcopy(expected)}
    verify_sqlite_runtime_pin(pin)
    probe_env = dict(environment, LC_ALL='C')
    result = subprocess.run(['ldd', str(executable)], env=probe_env, capture_output=True, text=True)
    require(result.returncode == 0, f'cannot inspect executable SQLite loader: {result.stderr}')
    resolved = re.findall(r'^\s*libsqlite3\.so(?:\.[0-9]+)*\s+=>\s+(\S+)\s+\(',
                          result.stdout, re.MULTILINE)
    require(len(resolved) == 1, 'executable does not resolve exactly one SQLite DSO')
    loader_path = Path(resolved[0])
    require(loader_path.resolve(strict=True) == Path(expected['link_library_path']),
            'executable SQLite loader path differs from accepted reference')
    verify_sqlite_runtime_pin(pin)
    pin.update(executable=str(Path(executable).resolve()), executable_sha256=runner.sha(Path(executable)),
               loader_path=str(loader_path), canonical_path=str(loader_path.resolve(strict=True)))
    return pin


def write_json(path, document):
    path.parent.mkdir(parents=True, exist_ok=True)
    temporary = path.with_suffix(path.suffix + '.tmp')
    temporary.write_text(json.dumps(document, indent=2, allow_nan=False) + '\n')
    temporary.replace(path)


def schedule(full_rounds, million_rounds):
    """Each item is one process; ULMDB pair members stay adjacent."""
    result = []
    for number in range(full_rounds):
        filesystems = ['disk', 'ram'] if number % 2 == 0 else ['ram', 'disk']
        for index, filesystem in enumerate(filesystems):
            arms = ['baseline', 'candidate'] if (number + index) % 2 == 0 else ['candidate', 'baseline']
            for arm in arms:
                result.append({'suite': 'full', 'filesystem': filesystem, 'round': number,
                               'engine': 'both', 'arm': arm})
    engines = ['ulmdb', 'lmdb', 'sqlite']
    for number in range(million_rounds):
        for engine in engines[number % 3:] + engines[:number % 3]:
            arms = (['baseline', 'candidate'] if number % 2 == 0 else ['candidate', 'baseline']) \
                if engine == 'ulmdb' else ['shared_control']
            for arm in arms:
                result.append({'suite': 'millions', 'round': number, 'engine': engine, 'arm': arm})
    for index, step in enumerate(result):
        step['number'] = index
    return result


def expected_hash(document, suite):
    hashes = {p['report']['binary_sha256'] if suite == 'full' else
              p['report']['build']['executable_sha256'] for p in document['profiles']}
    require(len(hashes) == 1, f'archived {suite} capture mixes executables')
    return hashes.pop()


def preserve_executable(original, target, expected=None):
    require(original.is_file() and os.access(original, os.X_OK), f'not executable: {original}')
    before = runner.sha(original)
    require(expected is None or expected == before, f'archived executable digest mismatch: {original}')
    target.parent.mkdir(parents=True, exist_ok=True)
    shutil.copy2(original, target)
    require(runner.sha(target) == before == runner.sha(original), f'executable changed while copied: {original}')
    target.chmod(0o555)
    return {'original': str(original), 'path': str(target), 'sha256': before}


def full_signature(report):
    return {
        'config': report['config'], 'dataset': report['dataset_sha256'], 'queries': report['query_sha256'],
        'reads': [{k: case[k] for k in ['name', 'queries', 'hits', 'matched_lengths',
                                      'expected_checksum', 'query_indices']} for case in report['reads']],
        'writes': [{k: case[k] for k in ['name', 'records_per_transaction',
                                       'commits_per_aggregate_phase', 'separate_latency_phase_commits']}
                   | {'traces': [t['trace_sha256'] for t in case['trials']]}
                   for case in report['writes']],
    }


def validate_full(report, fixture, expected, executable_hash, source):
    require(report['semantic_validation_passed'] is True and report['timing_enabled'] is True,
            'full semantic validation/timing failed')
    require(report['backend'] == expected['backend'] and report['experimental'] == expected['experimental'],
            'full backend/feature substitution')
    require(report['native_flags'] == 0 and report['native_version'] == expected['native_version']
            and report['native_build_info'] == expected['native_build_info'], 'full LMDB flags/version/build changed')
    require(report['binary_sha256'] == executable_hash, 'full executable digest mismatch')
    require(full_signature(report) == full_signature(expected), 'full workload/traces/config changed')
    require(len(report['reads']) == 13 and len(report['writes']) == 2, 'full case inventory incomplete')
    require(all(report['cold'][key] is True for key in
                ['old_reader_survived_environment_drop', 'escaped_value_pointer_unchanged',
                 'reopened_value_uses_different_live_mapping']), 'full cold/borrow oracle failed')
    embedded = report['source_hashes']
    require(embedded and all(source.get(path) == digest for path, digest in embedded.items()),
            'embedded full executable source provenance disagrees with selected source snapshot')
    raw = (fixture / report['raw_latency_file']).resolve()
    require(raw.is_relative_to(fixture.resolve()) and raw.is_file(), 'raw latency path outside fixture/missing')
    require(runner.sha(raw) == report['raw_latency_sha256'], 'raw latency file digest mismatch')
    contents = raw.read_bytes()
    import hashlib
    for read, cases in [(True, report['reads']), (False, report['writes'])]:
        for case in cases:
            require(len(case['trials']) == report['config']['trials'], 'full trial count mismatch')
            for trial in case['trials']:
                for engine in ['ulmdb', 'lmdb']:
                    span = trial[engine]['raw_latency']
                    count = case['queries'] if read else case['separate_latency_phase_commits']
                    require(span['count'] == count and span['offset_bytes'] % 8 == 0,
                            'raw trial sample count/alignment differs')
                    start, length = span['offset_bytes'], span['count'] * 8
                    require(start >= 0 and start + length <= len(contents), 'raw trial extent outside file')
                    require(hashlib.sha256(contents[start:start + length]).hexdigest() == span['sha256'],
                            'raw trial sample digest mismatch')
    return raw


def validate_millions(report, expected, config, executable_hash):
    engine = report['engine']
    require(report.get('status') == 'complete' and report['validation']['passed'] is True,
            'million workload failed/incomplete')
    require(report['backend'] == expected['backend'] and report['build']['resident_profile']
            == expected['build']['resident_profile'], 'million backend/feature substitution')
    require(report['build']['executable_sha256'] == executable_hash, 'million executable digest mismatch')
    require(report['config'] == config and report['workload'] == expected['workload'],
            'million workload/config changed')
    require(all(report['validation'][key] is True for key in
                ['initial_all_values_and_misses', 'timed_read_hits_and_checksum',
                 'final_all_values_and_misses', 'reopened_all_values_and_misses',
                 'exact_row_count']), 'million complete value/cursor/reopen oracles failed')
    rows = {row['operation']: row for row in report['rows']}
    require(len(rows) == len(report['rows']) == 3 and set(rows) == {'load', 'read', 'update'},
            'million operation inventory incomplete')
    for operation, count_key in [('load', 'keys'), ('read', 'reads'), ('update', 'updates')]:
        row = rows[operation]
        count = config[count_key]
        transactions = 1 if operation == 'read' else (count + config['batch'] - 1) // config['batch']
        require(row['engine'] == engine and row['operations'] == count and row['transactions'] == transactions
                and row['elapsed_ns'] > 0, 'million operation denominator mismatch')
    if engine == 'lmdb':
        require(report['settings']['flags'] == 0 and report['versions'] == expected['versions'],
                'million LMDB flags/version changed')
    if engine == 'sqlite':
        require(report['settings']['journal_mode'].lower() == 'wal'
                and str(report['settings']['synchronous_numeric']) == '2', 'SQLite durability changed')
        require(set(report['versions']) == set(expected['versions']) == SQLITE_VERSION_KEYS
                and all(isinstance(value, str) and value for value in report['versions'].values())
                and re.fullmatch('[0-9a-f]{64}', report['versions']['link_library_sha256']) is not None
                and report['versions'] == expected['versions'],
                'million SQLite runtime version/source_id/link path/hash changed')
    require(report['scope']['write_durability'] == expected['scope']['write_durability'],
            'reported write durability scope changed')


def captures(reference, arm, shared, full_rounds, million_rounds, source, executable_info):
    result = {}
    for suite, original in reference.items():
        document = copy.deepcopy({k: v for k, v in original.items() if k not in
                                  ['profiles', 'paired_baseline', 'paired_campaign', 'comparison_scope']})
        document.update(shared, profiles=[], source_sha256=source,
                        rounds=full_rounds if suite == 'full' else million_rounds)
        document['campaign'] = {
            'arm': arm, 'private': True, 'original_capture_generated_at': original['generated_at'],
            'executable': executable_info[arm][suite],
            'controls': ('millions LMDB/SQLite observations are run once from candidate executable and '
                         'shared identically across arms; they are controls, not independent arm comparisons'),
            'source_scope': ('source_sha256 describes this arm target executable input context; '
                             'shared million controls use candidate source and executable'),
        }
        result[suite] = document
    return result


def main(argv=None):
    parser = argparse.ArgumentParser(description=__doc__, formatter_class=argparse.RawDescriptionHelpFormatter)
    parser.add_argument('baseline', type=Path, help='preserved full.json, millions.json and both executables')
    parser.add_argument('--candidate-full', type=Path, required=True)
    parser.add_argument('--candidate-millions', type=Path, required=True)
    parser.add_argument('--candidate-source', type=Path, help='run.py snapshot JSON saved immediately after candidate build')
    parser.add_argument('--output', type=Path, required=True, help='new private campaign directory')
    parser.add_argument('--cpu', type=int, help='defaults to archived baseline CPU')
    parser.add_argument('--rounds', type=int, help='full process pairs/filesystem; cannot reduce original rounds')
    parser.add_argument('--million-rounds', type=int, help='million ULMDB pairs; cannot reduce original rounds')
    parser.add_argument('--ram-dir', type=Path, default=Path('/dev/shm'))
    parser.add_argument('--keep-fixtures', action='store_true')
    parser.add_argument('--plan-only', action='store_true', help='prepare immutable inputs/plan, run no benchmarks')
    args = parser.parse_args(argv)
    output, ram_root, manifest = args.output.resolve(), None, None
    documents = None
    try:
        require(sys.platform == 'linux', 'campaign requires Linux')
        reference, input_files = comparison.load_capture(args.baseline.resolve())
        require(set(reference) == {'full', 'millions'}, 'baseline needs both complete benchmark captures')
        comparison.flatten(reference)  # Reject invalid/incomplete baseline before any execution.
        require(reference['full']['cpu'] == reference['millions']['cpu'], 'baseline suites used different CPUs')
        cpu = args.cpu if args.cpu is not None else reference['full']['cpu']
        require(cpu in os.sched_getaffinity(0), 'selected CPU is not available')
        full_rounds = args.rounds if args.rounds is not None else reference['full']['rounds']
        million_rounds = args.million_rounds if args.million_rounds is not None else reference['millions']['rounds']
        require(full_rounds >= reference['full']['rounds'] and million_rounds >= reference['millions']['rounds'],
                'campaign cannot reduce archived baseline repetition counts')
        require(args.ram_dir.is_dir() and runner.filesystem_info(args.ram_dir)['type'] in ['tmpfs', 'ramfs'],
                '--ram-dir must be a real mounted tmpfs or ramfs')
        require(not output.exists(), 'campaign output already exists; use a fresh directory (no result replacement)')
        output.mkdir(parents=True)
        before = runner.snapshot()
        if args.candidate_source:
            require(json.loads(args.candidate_source.read_text()) == before,
                    'candidate build source manifest differs from current source')
        executables = {arm: {} for arm in ['baseline', 'candidate']}
        for arm in executables:
            for suite, name in [('full', 'ulmdb-engine-benchmark'), ('millions', 'ulmdb-millions')]:
                original = (args.baseline / name).resolve() if arm == 'baseline' else (
                    args.candidate_full if suite == 'full' else args.candidate_millions).resolve()
                executables[arm][suite] = preserve_executable(
                    original, output / arm / 'binaries' / name,
                    expected_hash(reference[suite], suite) if arm == 'baseline' else None)
        expected_full = reference['full']['profiles'][0]['report']
        expected_millions = {p['report']['engine']: p['report'] for p in reference['millions']['profiles']}
        require(reference['full']['source_sha256'] == reference['millions']['source_sha256'],
                'archived target suites do not share one source snapshot')
        for suite, document in reference.items():
            write_json(output / 'reference' / (suite + '.json'), document)
        private_tools = {str(p.relative_to(runner.ROOT)): runner.sha(p)
                         for p in [Path(__file__).resolve(), HERE / 'compare.py']}
        plan = schedule(full_rounds, million_rounds)
        manifest = {
            'schema': 1, 'status': 'planned', 'started_at': datetime.datetime.now(datetime.timezone.utc).isoformat(),
            'baseline_inputs': input_files, 'candidate_source_build_manifest':
                str(args.candidate_source.resolve()) if args.candidate_source else None,
            'candidate_build_scope': ('caller-provided build snapshot checked' if args.candidate_source else
                'caller built binaries; full embedded hashes checked after run; million build/source binding is caller responsibility'),
            'source_before': before, 'private_tools_sha256': private_tools, 'executables': executables,
            'cpu': cpu, 'full_rounds': full_rounds, 'million_rounds': million_rounds,
            'plan': plan, 'completed': [], 'controls_shared_between_arms': True,
            'failure_policy': 'stop at first failure; no retry, exclusion, fallback, or public report replacement',
        }
        write_json(output / 'campaign.json', manifest)
        if args.plan_only:
            print(f'Prepared {len(plan)} process steps; no benchmarks run: {output / "campaign.json"}')
            return 0
        ram_root = Path(tempfile.mkdtemp(prefix='ulmdb-paired-', dir=args.ram_dir)).resolve()
        (output / 'fixtures').mkdir()
        shared = {'generated_at': manifest['started_at'], 'machine': runner.machine(), 'cpu': cpu,
                  'filesystems': {'disk': runner.filesystem_info(output / 'fixtures'),
                                  'ram': runner.filesystem_info(ram_root)}}
        documents = {arm: captures(reference, arm, shared, full_rounds, million_rounds,
                                  reference['full']['source_sha256'] if arm == 'baseline' else before,
                                  executables) for arm in ['baseline', 'candidate']}
        env = {k: v for k, v in os.environ.items() if not k.startswith(
            ('ULMDB_BENCH_', 'ULMDB_TEST_', 'ULMDB_RESIDENT_', 'ULMDB_HYBRID_'))}
        env['PYTHONDONTWRITEBYTECODE'] = '1'
        config = reference['millions']['config']
        manifest.update(status='running', ram_work=str(ram_root), shared_metadata=shared)
        write_json(output / 'campaign.json', manifest)
        for step in plan:
            suite, arm = step['suite'], step['arm']
            executable_arm = 'candidate' if arm == 'shared_control' else arm
            info = executables[executable_arm][suite]
            filesystem = step.get('filesystem', reference['millions']['storage'])
            label = f"{step['number']:03d}-{suite}-{filesystem}-{step['round']}-{step['engine']}-{arm}"
            location = ram_root if filesystem == 'ram' else output / 'fixtures'
            fixture = location / label
            print(f"[{step['number'] + 1}/{len(plan)}] {label}", flush=True)
            started = time.monotonic()
            argv_child = [info['path'], 'bench', fixture] if suite == 'full' else [
                info['path'], '--engine', step['engine'], '--dir', fixture, '--measure']
            if suite == 'millions':
                for key, value in config.items():
                    argv_child += ['--' + key.replace('_', '-'), str(value)]
            try:
                report = runner.child_json(argv_child, fixture, env, cpu)
            finally:
                # Keep even failed/partial process logs on ordinary storage.
                for suffix in ['.stdout.json', '.stderr.log']:
                    log = fixture.parent / (fixture.name + suffix)
                    if log.exists():
                        (output / 'logs').mkdir(exist_ok=True)
                        shutil.copy2(log, output / 'logs' / log.name)
                        log.unlink()
                failure = fixture / 'resident-failure.json'
                if failure.exists():
                    (output / 'logs').mkdir(exist_ok=True)
                    shutil.copy2(failure, output / 'logs' / (label + '.resident-failure.json'))
            source = reference['full']['source_sha256'] if executable_arm == 'baseline' else before
            profile = {'round': step['round'], 'pair': step['round'], 'report': report,
                       'campaign_step': step['number'], 'source_arm': executable_arm,
                       'executable_sha256': info['sha256'], 'shared_control': arm == 'shared_control'}
            if suite == 'full':
                raw = validate_full(report, fixture, expected_full, info['sha256'], source)
                (output / 'traces').mkdir(exist_ok=True)
                destination = output / 'traces' / (label + '.u64le')
                shutil.copy2(raw, destination)
                profile.update(filesystem=filesystem, campaign_raw_latency_file=str(destination.relative_to(output)))
            else:
                require(report['engine'] == step['engine'], 'million process engine substitution')
                validate_millions(report, expected_millions[step['engine']], config, info['sha256'])
            target_arms = ['baseline', 'candidate'] if arm == 'shared_control' else [arm]
            for target_arm in target_arms:
                documents[target_arm][suite]['profiles'].append(copy.deepcopy(profile))
                write_json(output / target_arm / (suite + '.partial.json'), documents[target_arm][suite])
            manifest['completed'].append(dict(step, elapsed_wall_seconds=time.monotonic() - started,
                                              fixture=str(fixture), report_validated=True))
            write_json(output / 'campaign.json', manifest)
            if not args.keep_fixtures:
                shutil.rmtree(fixture)
        after = runner.snapshot()
        manifest['source_after'] = after
        require(after == before, 'source changed during campaign; private evidence retained, campaign rejected')
        require(all(runner.sha(runner.ROOT / path) == digest for path, digest in private_tools.items()),
                'campaign/comparison tool changed during execution')
        for arm in executables:
            for info in executables[arm].values():
                require(runner.sha(Path(info['path'])) == info['sha256'], 'preserved executable changed during campaign')
        result = comparison.compare(documents['baseline'], documents['candidate'], paired=True,
                                    min_repeats=min(full_rounds, million_rounds))
        for arm in documents:
            for suite, document in documents[arm].items():
                write_json(output / arm / (suite + '.json'), document)
                (output / arm / (suite + '.partial.json')).unlink()
        result.update(baseline_inputs=[{'path': str(output / 'baseline')}],
                      candidate_inputs=[{'path': str(output / 'candidate')}],
                      campaign_manifest=str(output / 'campaign.json'))
        write_json(output / 'comparison.json', result)
        (output / 'comparison.md').write_text(comparison.markdown(result))
        manifest.update(status='complete', finished_at=datetime.datetime.now(datetime.timezone.utc).isoformat(),
                        source_unchanged=True, comparison_summary=result['summary'])
        write_json(output / 'campaign.json', manifest)
        print(json.dumps(result['summary'], indent=2))
        if not args.keep_fixtures:
            shutil.rmtree(ram_root)
        return 0 if result['summary']['screen_clear'] else 1
    except (OSError, ValueError, RuntimeError, KeyError, TypeError, AssertionError) as error:
        if manifest is not None:
            manifest.update(status='failed', error=str(error),
                            failed_at=datetime.datetime.now(datetime.timezone.utc).isoformat())
            if ram_root is not None:
                manifest['retained_ram_work'] = str(ram_root)
            write_json(output / 'campaign.json', manifest)
        print(f'campaign failed; all evidence retained at {output}: {error}', file=sys.stderr)
        return 2


if __name__ == '__main__':
    sys.exit(main())
