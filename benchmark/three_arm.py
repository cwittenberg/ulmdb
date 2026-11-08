#!/usr/bin/env python3
"""Run private initial/accepted/candidate benchmarks without duplicate targets.

Uses benchmark/campaign.py's existing validation, capture and binary helpers.
Build candidate binaries separately, then supply immutable initial and accepted
capture directories. All three target arms run once per block; comparison-engine
controls run once and are explicitly shared. No public reports are changed.
"""
from __future__ import annotations

import argparse
import copy
import datetime
import importlib.util
import json
import os
from pathlib import Path
import shutil
import sys
import tempfile
import time

BENCHMARK = next(parent for parent in Path(__file__).resolve().parents
                 if (parent / 'campaign.py').is_file() and (parent / 'run.py').is_file())
SPEC = importlib.util.spec_from_file_location('ulmdb_two_arm_campaign', BENCHMARK / 'campaign.py')
base = importlib.util.module_from_spec(SPEC)
SPEC.loader.exec_module(base)
runner, comparison, require = base.runner, base.comparison, base.require
ARMS = ['initial', 'accepted', 'candidate']


def schedule(full_rounds=4, million_rounds=3):
    """Rotate all three target positions; keep each three-process block adjacent."""
    result = []
    for number in range(full_rounds):
        filesystems = ['disk', 'ram'] if number % 2 == 0 else ['ram', 'disk']
        for filesystem in filesystems:
            shift = (number + (filesystem == 'ram')) % len(ARMS)
            for arm in ARMS[shift:] + ARMS[:shift]:
                result.append({'suite': 'full', 'filesystem': filesystem,
                               'round': number, 'engine': 'both', 'arm': arm})
    engines = ['ulmdb', 'lmdb', 'sqlite']
    for number in range(million_rounds):
        for engine in engines[number % 3:] + engines[:number % 3]:
            shift = number % len(ARMS)
            arms = ARMS[shift:] + ARMS[:shift] if engine == 'ulmdb' else ['shared_control']
            for arm in arms:
                result.append({'suite': 'millions', 'round': number, 'engine': engine, 'arm': arm})
    return [dict(step, number=index) for index, step in enumerate(result)]


def target_hash(document, suite):
    # Prior campaign baseline captures may contain shared controls built from a
    # different binary. Pin the ULMDB target, never infer its identity from them.
    profiles = document['profiles'] if suite == 'full' else [
        p for p in document['profiles'] if p['report']['engine'] == 'ulmdb']
    require(profiles, f'{suite} has no target observations')
    return base.expected_hash(dict(document, profiles=profiles), suite)


def archived_executable(directory, name):
    direct, nested = directory / name, directory / 'binaries' / name
    existing = [p for p in [direct, nested] if p.is_file()]
    require(len(existing) == 1, f'need exactly one preserved {name} in {directory} or binaries/')
    return existing[0].resolve()


def check_references(references, sqlite_runtime=None):
    signatures = {}
    for arm, reference in references.items():
        require(set(reference) == {'full', 'millions'}, f'{arm}: both full and millions captures required')
        _, signatures[arm], _ = comparison.flatten(reference)
        require(reference['full']['source_sha256'] == reference['millions']['source_sha256'],
                f'{arm}: target suites do not have one source snapshot')
        require(reference['full']['cpu'] == reference['millions']['cpu'], f'{arm}: suite CPUs differ')
        for suite, document in reference.items():
            target_hash(document, suite)
    require(signatures['initial'] == signatures['accepted'], 'initial/accepted workloads differ')
    require(references['initial']['full']['cpu'] == references['accepted']['full']['cpu'],
            'initial/accepted archived CPUs differ')
    for key in ['native_version', 'native_build_info']:
        require(references['initial']['full']['profiles'][0]['report'][key]
                == references['accepted']['full']['profiles'][0]['report'][key],
                f'initial/accepted full comparison-engine {key} differs')
    native = {arm: {p['report']['engine']: p['report']
                   for p in reference['millions']['profiles']} for arm, reference in references.items()}
    require(native['initial']['lmdb']['versions'] == native['accepted']['lmdb']['versions'],
            'initial/accepted million lmdb versions differ')
    sqlite = {arm: native[arm]['sqlite']['versions'] for arm in references}
    for arm, reference in references.items():
        require(all(p['report']['versions'] == sqlite[arm] for p in reference['millions']['profiles']
                    if p['report']['engine'] == 'sqlite'), f'{arm}: archived SQLite identities differ')
    if sqlite_runtime is None:
        require(sqlite['initial'] == sqlite['accepted'],
                'historical SQLite hash difference requires verified accepted runtime context')
        return None
    return base.sqlite_reference_context(sqlite['initial'], sqlite['accepted'], sqlite_runtime['versions'])


def retain_logs(fixture, output, label):
    """Called in finally, including failed and partially written child reports."""
    for suffix in ['.stdout.json', '.stderr.log']:
        original = fixture.parent / (fixture.name + suffix)
        if original.exists():
            destination = output / 'logs' / original.name
            destination.parent.mkdir(exist_ok=True)
            shutil.copy2(original, destination)
            original.unlink()
    failure = fixture / 'resident-failure.json'
    if failure.exists():
        destination = output / 'logs' / (label + '.resident-failure.json')
        destination.parent.mkdir(exist_ok=True)
        shutil.copy2(failure, destination)


def verify_guards(before, executables, tools, traces, sqlite_context=None, env=None):
    after = runner.snapshot()
    require(after == before, 'source changed during campaign; captures rejected, private evidence retained')
    require(all(runner.sha(Path(path)) == digest for path, digest in tools.items()),
            'campaign, shared helper or comparison tool changed during execution')
    for arm in executables:
        for info in executables[arm].values():
            require(runner.sha(Path(info['path'])) == info['sha256'], f'{arm} preserved executable changed')
    require(all(runner.sha(Path(path)) == digest for path, digest in traces.items()),
            'preserved raw latency trace changed during campaign')
    if sqlite_context is not None:
        base.verify_sqlite_runtime_pin(sqlite_context['runtime_pin'])
        for suites in executables.values():
            base.inspect_sqlite_runtime(suites['millions']['path'], sqlite_context['runtime'], env)
    return after


def main(argv=None):
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument('--initial', type=Path, required=True)
    parser.add_argument('--accepted', type=Path, required=True)
    parser.add_argument('--candidate-full', type=Path, required=True)
    parser.add_argument('--candidate-millions', type=Path, required=True)
    parser.add_argument('--candidate-source', type=Path,
                        help='optional run.py snapshot JSON captured immediately after build')
    parser.add_argument('--output', type=Path, required=True, help='new private directory')
    parser.add_argument('--cpu', type=int)
    parser.add_argument('--rounds', type=int, default=4)
    parser.add_argument('--million-rounds', type=int, default=3)
    parser.add_argument('--ram-dir', type=Path, default=Path('/dev/shm'))
    parser.add_argument('--keep-fixtures', action='store_true')
    parser.add_argument('--plan-only', action='store_true')
    args = parser.parse_args(argv)
    output, manifest, ram_root = args.output.resolve(), None, None
    current_step, fixture = None, None
    try:
        require(sys.platform == 'linux', 'campaign requires Linux')
        directories = {'initial': args.initial.resolve(), 'accepted': args.accepted.resolve()}
        references, inputs = {}, {}
        for arm, directory in directories.items():
            references[arm], inputs[arm] = comparison.load_capture(directory)
        env = {k: v for k, v in os.environ.items() if not k.startswith(
            ('ULMDB_BENCH_', 'ULMDB_TEST_', 'ULMDB_RESIDENT_', 'ULMDB_HYBRID_'))}
        env['PYTHONDONTWRITEBYTECODE'] = '1'
        accepted_sqlite = next(p['report']['versions']
                              for p in references['accepted']['millions']['profiles']
                              if p['report']['engine'] == 'sqlite')
        originals = {arm: archived_executable(directory, 'ulmdb-millions')
                     for arm, directory in directories.items()}
        originals['candidate'] = args.candidate_millions.resolve()
        runtime = {arm: base.inspect_sqlite_runtime(path, accepted_sqlite, env)
                   for arm, path in originals.items()}
        sqlite_context = check_references(references, runtime['accepted'])
        sqlite_context.update(runtime_pin=runtime['accepted'], original_elf_resolutions=runtime)
        cpu = args.cpu if args.cpu is not None else references['initial']['full']['cpu']
        require(cpu in os.sched_getaffinity(0), 'selected CPU unavailable')
        require(args.rounds >= max(4, *[r['full']['rounds'] for r in references.values()])
                and args.million_rounds >= max(3, *[r['millions']['rounds'] for r in references.values()]),
                'cannot reduce standard or archived reference repetitions')
        require(args.ram_dir.is_dir() and runner.filesystem_info(args.ram_dir)['type'] in ['tmpfs', 'ramfs'],
                '--ram-dir requires a real mounted tmpfs or ramfs')
        require(not output.exists(), 'output already exists; use a fresh directory')
        output.mkdir(parents=True)
        before = runner.snapshot()
        if args.candidate_source:
            require(json.loads(args.candidate_source.read_text()) == before,
                    'candidate build source snapshot differs from current source')
        sources = {arm: references[arm]['full']['source_sha256'] for arm in directories}
        sources['candidate'] = before
        executables = {arm: {} for arm in ARMS}
        for arm in ARMS:
            for suite, name in [('full', 'ulmdb-engine-benchmark'), ('millions', 'ulmdb-millions')]:
                original = archived_executable(directories[arm], name) if arm in directories else (
                    args.candidate_full if suite == 'full' else args.candidate_millions).resolve()
                executables[arm][suite] = base.preserve_executable(
                    original, output / arm / 'binaries' / name,
                    target_hash(references[arm][suite], suite) if arm in directories else None)
        sqlite_context['preserved_elf_resolutions'] = {
            arm: base.inspect_sqlite_runtime(executables[arm]['millions']['path'],
                                              sqlite_context['runtime'], env) for arm in ARMS}
        for arm, reference in references.items():
            for suite, document in reference.items():
                base.write_json(output / 'reference' / arm / (suite + '.json'), document)
        tool_paths = [Path(__file__).resolve(), BENCHMARK / 'campaign.py',
                      BENCHMARK / 'compare.py', BENCHMARK / 'run.py']
        tools = {str(path): runner.sha(path) for path in tool_paths}
        plan = schedule(args.rounds, args.million_rounds)
        manifest = {
            'schema': 1, 'status': 'planned', 'started_at': datetime.datetime.now(datetime.timezone.utc).isoformat(),
            'arms': ARMS, 'reference_inputs': inputs, 'source_before': before,
            'arm_sources': sources, 'executables': executables, 'private_tools_sha256': tools,
            'cpu': cpu, 'full_rounds': args.rounds, 'million_rounds': args.million_rounds,
            'plan': plan, 'completed': [], 'raw_traces_sha256': {}, 'controls_shared_between_arms': True,
            'sqlite_runtime_context': sqlite_context,
            'candidate_build_scope': ('caller build snapshot supplied and checked' if args.candidate_source else
                'caller built binaries; full embedded hashes checked; million build/source binding remains caller responsibility'),
            'failure_policy': 'stop at first failure; no retries, dropped cells, fallback, or public report changes',
            'acceptance_policy': 'both comparisons preserve every adverse estimate; no automatic acceptance or policy exceptions',
            'pairing_scope': 'three neighboring target processes per filesystem/round or million engine group; '
                             'same round IDs associate process blocks, not independent trial samples',
        }
        base.write_json(output / 'campaign.json', manifest)
        if args.plan_only:
            print(f'Prepared {len(plan)} process steps; no benchmarks executed: {output}')
            return 0
        ram_root = Path(tempfile.mkdtemp(prefix='ulmdb-three-arm-', dir=args.ram_dir)).resolve()
        (output / 'fixtures').mkdir()
        shared = {'generated_at': manifest['started_at'], 'machine': runner.machine(), 'cpu': cpu,
                  'filesystems': {'disk': runner.filesystem_info(output / 'fixtures'),
                                  'ram': runner.filesystem_info(ram_root)}}
        # Candidate expectations use last accepted workload/backend identity;
        # its actual source binding is the fresh candidate snapshot above.
        arm_references = dict(references, candidate=references['accepted'])
        documents = {arm: base.captures(arm_references[arm], arm, shared, args.rounds, args.million_rounds,
                                        sources[arm], executables) for arm in ARMS}
        for arm in documents:
            for document in documents[arm].values():
                document['campaign'].update(arms=ARMS, reference_arm='accepted' if arm == 'candidate' else arm)
        expected_full = {arm: arm_references[arm]['full']['profiles'][0]['report'] for arm in ARMS}
        expected_millions = {arm: {p['report']['engine']: p['report']
                                   for p in arm_references[arm]['millions']['profiles']} for arm in ARMS}
        config = references['initial']['millions']['config']
        storage = references['initial']['millions']['storage']
        manifest.update(status='running', ram_work=str(ram_root), shared_metadata=shared)
        base.write_json(output / 'campaign.json', manifest)
        for step in plan:
            current_step = step
            base.verify_sqlite_runtime_pin(sqlite_context['runtime_pin'])
            suite, arm = step['suite'], step['arm']
            executable_arm = 'candidate' if arm == 'shared_control' else arm
            info = executables[executable_arm][suite]
            filesystem = step.get('filesystem', storage)
            label = f"{step['number']:03d}-{suite}-{filesystem}-{step['round']}-{step['engine']}-{arm}"
            fixture = (ram_root if filesystem == 'ram' else output / 'fixtures') / label
            print(f"[{step['number'] + 1}/{len(plan)}] {label}", flush=True)
            started = time.monotonic()
            child_argv = [info['path'], 'bench', fixture] if suite == 'full' else [
                info['path'], '--engine', step['engine'], '--dir', fixture, '--measure']
            if suite == 'millions':
                base.inspect_sqlite_runtime(info['path'], sqlite_context['runtime'], env)
                for key, value in config.items():
                    child_argv += ['--' + key.replace('_', '-'), str(value)]
            try:
                report = runner.child_json(child_argv, fixture, env, cpu)
            finally:
                retain_logs(fixture, output, label)
            base.verify_sqlite_runtime_pin(sqlite_context['runtime_pin'])
            profile = {'round': step['round'], 'pair': step['round'], 'report': report,
                       'campaign_step': step['number'], 'source_arm': executable_arm,
                       'executable_sha256': info['sha256'], 'shared_control': arm == 'shared_control',
                       'three_arm_process_block': step['round']}
            if suite == 'full':
                raw = base.validate_full(report, fixture, expected_full[arm], info['sha256'], sources[arm])
                destination = output / 'traces' / (label + '.u64le')
                destination.parent.mkdir(exist_ok=True)
                shutil.copy2(raw, destination)
                require(runner.sha(destination) == report['raw_latency_sha256'], 'copied raw trace digest mismatch')
                destination.chmod(0o444)
                manifest['raw_traces_sha256'][str(destination)] = report['raw_latency_sha256']
                profile.update(filesystem=filesystem, campaign_raw_latency_file=str(destination.relative_to(output)))
            else:
                require(report['engine'] == step['engine'], 'million process engine substitution')
                base.validate_millions(report, expected_millions[executable_arm][step['engine']],
                                       config, info['sha256'])
            for target_arm in ARMS if arm == 'shared_control' else [arm]:
                documents[target_arm][suite]['profiles'].append(copy.deepcopy(profile))
                base.write_json(output / target_arm / (suite + '.partial.json'), documents[target_arm][suite])
            manifest['completed'].append(dict(step, elapsed_wall_seconds=time.monotonic() - started,
                                              fixture=str(fixture), report_validated=True))
            base.write_json(output / 'campaign.json', manifest)
            if not args.keep_fixtures:
                shutil.rmtree(fixture)
            fixture = None
        manifest['source_after'] = verify_guards(before, executables, tools, manifest['raw_traces_sha256'],
                                                sqlite_context, env)
        comparisons = {}
        for reference_arm in ['initial', 'accepted']:
            result = comparison.compare(documents[reference_arm], documents['candidate'], paired=True,
                                        min_repeats=min(args.rounds, args.million_rounds))
            result.update(reference_arm=reference_arm, candidate_arm='candidate',
                          campaign_manifest=str(output / 'campaign.json'),
                          reference_inputs=[{'path': str(output / reference_arm)}],
                          candidate_inputs=[{'path': str(output / 'candidate')}])
            comparisons[reference_arm] = result
        # Only complete, fully validated, source-stable campaigns get final captures.
        for arm in documents:
            comparison.flatten(documents[arm])
            for suite, document in documents[arm].items():
                base.write_json(output / arm / (suite + '.json'), document)
                (output / arm / (suite + '.partial.json')).unlink()
        for reference_arm, result in comparisons.items():
            prefix = output / (reference_arm + '-vs-candidate')
            base.write_json(prefix.with_suffix('.json'), result)
            prefix.with_suffix('.md').write_text(comparison.markdown(result))
        summaries = {arm: result['summary'] for arm, result in comparisons.items()}
        manifest.update(status='complete', finished_at=datetime.datetime.now(datetime.timezone.utc).isoformat(),
                        source_unchanged=True, comparison_summaries=summaries)
        base.write_json(output / 'campaign.json', manifest)
        print(json.dumps(summaries, indent=2))
        if not args.keep_fixtures:
            shutil.rmtree(ram_root)
        return 0 if all(s['screen_clear'] for s in summaries.values()) else 1
    except (OSError, ValueError, RuntimeError, KeyError, TypeError, AssertionError) as error:
        if manifest is not None:
            manifest.update(status='failed', error=str(error), failed_step=current_step,
                            failed_at=datetime.datetime.now(datetime.timezone.utc).isoformat())
            if ram_root is not None:
                manifest['retained_ram_work'] = str(ram_root)
            if fixture is not None:
                manifest['retained_failed_fixture'] = str(fixture)
            base.write_json(output / 'campaign.json', manifest)
        print(f'three-arm campaign failed; evidence retained at {output}: {error}', file=sys.stderr)
        return 2


if __name__ == '__main__':
    sys.exit(main())
