"""Build, run and render the public Linux benchmark suites."""
from pathlib import Path
import argparse
import datetime
import hashlib
import json
import os
import shutil
import subprocess
import sys
import tempfile
import time

HERE = Path(__file__).resolve().parent
ROOT = HERE.parent
RUNTIME = ['ulmdb', 'ulmdb-core', 'ulmdb-storage', 'ulmdb-kv', 'ulmdb-prefix']


def sha(path):
    digest = hashlib.sha256()
    with path.open('rb') as stream:
        for data in iter(lambda: stream.read(1 << 20), b''):
            digest.update(data)
    return digest.hexdigest()


def snapshot():
    files = [ROOT / p for p in ['Cargo.toml', 'Cargo.lock', 'rust-toolchain.toml']]
    for directory in [*[ROOT / 'crates' / n for n in RUNTIME],
                      HERE / 'engine', HERE / 'millions', HERE / 'support/lmdb']:
        files.extend(p for p in directory.rglob('*') if p.is_file()
                     and not any(x in p.relative_to(directory).parts for x in ['target', '__pycache__'])
                     and (p.suffix in ['.rs', '.toml', '.lock', '.c', '.h'] or p.name in ['LICENSE', 'COPYRIGHT']))
    files.extend([HERE / 'run.py', HERE / 'report.py', HERE / 'requirements.txt', HERE / 'run.sh'])
    return {str(p.relative_to(ROOT)): sha(p) for p in sorted(set(files))}


def machine():
    model = next((s.split(':', 1)[1].strip() for s in Path('/proc/cpuinfo').read_text().splitlines()
                  if s.startswith('model name')), os.uname().machine)
    return {'cpu_model': model, 'kernel': os.uname().release, 'architecture': os.uname().machine,
            'available_cpus': sorted(os.sched_getaffinity(0)), 'platform': sys.platform}


def filesystem_info(path):
    return {'type': subprocess.check_output(['stat', '-f', '-c', '%T', str(path)], text=True).strip(),
            'device': path.stat().st_dev}


def command(argv, log, env):
    log.parent.mkdir(parents=True, exist_ok=True)
    with log.open('w') as output:
        result = subprocess.run(argv, cwd=ROOT, env=env, stdout=output, stderr=subprocess.STDOUT)
    if result.returncode:
        raise RuntimeError(f'command failed ({result.returncode}); retained log: {log}')


def build(package, target, features, log, env):
    build_env = dict(env, CARGO_TARGET_DIR=str(target), CARGO_BUILD_JOBS='1')
    for name in ['RUSTFLAGS', 'CARGO_ENCODED_RUSTFLAGS', 'RUSTC_WRAPPER', 'RUSTC_WORKSPACE_WRAPPER']:
        build_env.pop(name, None)
    args = ['cargo', '+1.94.0', 'build', '--locked', '--release', '--manifest-path', str(package / 'Cargo.toml')]
    if features:
        args += ['--features', features]
    print('Building ' + package.name, flush=True)
    command(args, log, build_env)


def plotting(env, log):
    location = HERE / '.cache/python'
    env['PYTHONPATH'] = str(location) + (os.pathsep + env['PYTHONPATH'] if env.get('PYTHONPATH') else '')
    if subprocess.run([sys.executable, '-c', 'import matplotlib'], env=env,
                      stdout=subprocess.DEVNULL, stderr=subprocess.DEVNULL).returncode:
        print('Installing benchmark-only plotting dependency', flush=True)
        command([sys.executable, '-m', 'pip', 'install', '--disable-pip-version-check',
                 '--target', str(location), '-r', str(HERE / 'requirements.txt')], log, env)


def child_json(argv, directory, env, cpu, allow_resident_failure=False):
    directory.parent.mkdir(parents=True, exist_ok=True)
    stdout, stderr = directory.parent / (directory.name + '.stdout.json'), directory.parent / (directory.name + '.stderr.log')
    with stdout.open('w') as out, stderr.open('w') as err:
        result = subprocess.run(['taskset', '-c', str(cpu), *map(str, argv)], cwd=ROOT, env=env,
                                stdout=out, stderr=err)
    if result.returncode:
        failure = directory / 'resident-failure.json'
        if allow_resident_failure and failure.is_file():
            report = json.loads(failure.read_text())
            assert report['engine'] == 'ulmdb' and report['status'] == 'failed'
            assert not report['validation']['passed']
            report['process_exit_code'] = result.returncode
            report.setdefault('build', {})['executable_sha256'] = sha(Path(argv[0]))
            return report
        raise RuntimeError(f'benchmark failed ({result.returncode}); no retry; inspect {stderr}')
    return json.loads(stdout.read_text())


def publish(name, document):
    target = HERE / 'results' / (name + '.json')
    target.parent.mkdir(parents=True, exist_ok=True)
    temporary = target.with_suffix('.json.tmp')
    temporary.write_text(json.dumps(document, indent=2) + '\n')
    temporary.replace(target)


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument('suite', nargs='?', choices=['all', 'full', 'millions', 'smoke'], default='all')
    parser.add_argument('--backend', choices=['default', 'resident'], default='default', help='ULMDB backend for both suites; resident never falls back to default')
    parser.add_argument('--rounds', type=int, default=4, help='full-suite fresh processes per filesystem')
    parser.add_argument('--million-rounds', type=int, default=3, help='rotated fresh runs per engine')
    parser.add_argument('--keys', type=int, default=1_000_000)
    parser.add_argument('--reads', type=int, default=10_000_000)
    parser.add_argument('--updates', type=int, default=1_000_000)
    parser.add_argument('--batch', type=int, default=1000)
    parser.add_argument('--seed', type=int, default=73)
    parser.add_argument('--lmdb-map-bytes', type=int, default=1 << 30)
    parser.add_argument('--cpu', type=int, default=min(os.sched_getaffinity(0)))
    parser.add_argument('--storage', choices=['disk', 'ram'], default='disk', help='million-suite filesystem')
    parser.add_argument('--work-dir', type=Path, default=HERE / 'work')
    parser.add_argument('--ram-dir', type=Path, default=Path('/dev/shm'))
    parser.add_argument('--keep-work', action='store_true')
    args = parser.parse_args()
    if sys.platform != 'linux' or args.cpu not in os.sched_getaffinity(0):
        parser.error('requires Linux and an available CPU')
    if min(args.rounds, args.million_rounds, args.keys, args.reads, args.updates, args.batch) < 1:
        parser.error('all operation counts, batches and repeat counts must be positive')
    if not args.ram_dir.is_dir():
        parser.error('RAM directory missing; set --ram-dir to a mounted RAM filesystem')
    if filesystem_info(args.ram_dir)['type'] not in ['tmpfs', 'ramfs']:
        parser.error('--ram-dir must be a real tmpfs or ramfs mount')
    need_full = args.suite in ['all', 'full', 'smoke']
    need_million = args.suite in ['all', 'millions', 'smoke']
    smoke = args.suite == 'smoke'
    args.work_dir.mkdir(parents=True, exist_ok=True)
    work = Path(tempfile.mkdtemp(prefix='run-', dir=args.work_dir))
    ram = Path(tempfile.mkdtemp(prefix='ulmdb-benchmark-', dir=args.ram_dir))
    env = {k: v for k, v in os.environ.items() if not k.startswith(('ULMDB_BENCH_', 'ULMDB_TEST_', 'ULMDB_RESIDENT_', 'ULMDB_HYBRID_'))}
    env['PYTHONDONTWRITEBYTECODE'] = '1'
    success = False
    pending = {}
    try:
        build_source = snapshot()
        full_exe = (HERE / 'target' / ('engine-' + args.backend) / 'release/ulmdb-engine-benchmark').resolve()
        million_target = HERE / 'target' / ('millions-' + args.backend)
        million_exe = (million_target / 'release/ulmdb-millions').resolve()
        if need_full:
            build(HERE / 'engine', HERE / 'target' / ('engine-' + args.backend),
                  'resident' if args.backend == 'resident' else '', work / 'build-full.log', env)
        if need_million:
            build(HERE / 'millions', million_target,
                  'resident' if args.backend == 'resident' else '', work / 'build-millions.log', env)
        if not smoke:
            plotting(env, work / 'plot-install.log')
        before = snapshot()
        assert before == build_source, 'benchmark source changed during build; no timing run started'
        config = {'keys': args.keys, 'reads': args.reads, 'updates': args.updates,
                  'batch': args.batch, 'seed': args.seed, 'lmdb_map_bytes': args.lmdb_map_bytes}
        if smoke:
            config.update(keys=1024, reads=10000, updates=1024, batch=64)
        shared = {'schema': 1, 'generated_at': datetime.datetime.now(datetime.timezone.utc).isoformat(),
                  'machine': machine(), 'cpu': args.cpu, 'source_sha256': before,
                  'filesystems': {'disk': filesystem_info(work), 'ram': filesystem_info(ram)}}
        pending = {}
        if need_full:
            profiles = []
            for round_number in range(1 if smoke else args.rounds):
                order = ['disk', 'ram'] if round_number % 2 == 0 else ['ram', 'disk']
                for filesystem in order:
                    path = (work if filesystem == 'disk' else ram) / f'full-{round_number}-{filesystem}'
                    print(f'Full suite: {filesystem}, run {round_number + 1}', flush=True)
                    report = child_json([full_exe, 'smoke' if smoke else 'bench', path], path, env, args.cpu)
                    assert report['semantic_validation_passed'] and report['timing_enabled'] is not smoke
                    expected = 'experimental_resident_compact_borrowed' if args.backend == 'resident' else 'default_file_legacy'
                    assert report['backend'] == expected, (report['backend'], expected)
                    assert report['native_flags'] == 0 and report['binary_sha256'] == sha(full_exe)
                    assert sha(path / report['raw_latency_file']) == report['raw_latency_sha256']
                    profiles.append({'filesystem': filesystem, 'round': round_number, 'report': report})
                    if not args.keep_work:
                        shutil.rmtree(path)
            pending['full'] = dict(shared, suite='full', backend=args.backend,
                                   rounds=1 if smoke else args.rounds, profiles=profiles)
        if need_million:
            profiles, identity = [], None
            engines = ['ulmdb', 'lmdb', 'sqlite']
            for round_number in range(1 if smoke else args.million_rounds):
                order = engines[round_number % 3:] + engines[:round_number % 3]
                for engine in order:
                    path = (ram if args.storage == 'ram' else work) / f'millions-{round_number}-{engine}'
                    print(f'Million suite: {engine}, run {round_number + 1}', flush=True)
                    argv = [million_exe, '--engine', engine, '--dir', path, '--measure']
                    for key, value in config.items():
                        argv += ['--' + key.replace('_', '-'), str(value)]
                    report = child_json(argv, path, env, args.cpu,
                                        allow_resident_failure=engine == 'ulmdb' and args.backend == 'resident')
                    failed = report.get('status') == 'failed'
                    assert report['engine'] == engine
                    assert (not report['validation']['passed']) if failed else report['validation']['passed']
                    if engine == 'ulmdb':
                        expected_backend = ('ulmdb::resident::Environment CompactV2 Borrowed'
                                            if args.backend == 'resident' else 'ulmdb::Environment default Legacy')
                        assert report['backend'] == expected_backend, 'ULMDB backend substitution'
                    assert report['config'] == config and report['build']['executable_sha256'] == sha(million_exe)
                    if identity is None:
                        identity = report['workload']
                    assert report['workload'] == identity, 'engine workloads differ'
                    rows = {r['operation']: r for r in report['rows']}
                    assert len(report['rows']) == len(rows)
                    if failed:
                        assert engine == 'ulmdb' and args.backend == 'resident'
                        assert set(rows).issubset({'load', 'read', 'update'})
                        assert set(rows) <= set(report['validation']['completed_phases'])
                    else:
                        assert len(rows) == 3 and set(rows) == {'load', 'read', 'update'}
                    for key, operation in [('keys', 'load'), ('reads', 'read'), ('updates', 'update')]:
                        if operation not in rows:
                            assert failed
                            continue
                        row = rows[operation]
                        assert row['engine'] == engine and row['operations'] == config[key] and row['elapsed_ns'] > 0
                        assert row['transactions'] == (1 if operation == 'read' else (config[key] + config['batch'] - 1) // config['batch'])
                    profiles.append({'round': round_number, 'report': report})
                    if not args.keep_work and not failed:
                        shutil.rmtree(path)
            pending['millions'] = dict(shared, suite='millions', config=config, storage=args.storage,
                                       ulmdb_backend=args.backend,
                                       rounds=1 if smoke else args.million_rounds, profiles=profiles)
        assert snapshot() == before, 'benchmark source changed during the run; results retained privately, not published'
        failed = [p for p in pending.get('millions', {}).get('profiles', [])
                  if p['report'].get('status') == 'failed'
                  or not p['report']['validation']['passed']]
        if failed:
            evidence = work / 'FAILED_RESULTS.json'
            evidence.write_text(json.dumps(pending, indent=2) + '\n')
            raise RuntimeError(f'{len(failed)} benchmark profiles failed; captured evidence: {evidence}; '
                               'published results unchanged')
        if smoke:
            assert all(p['report']['validation']['passed']
                       for p in pending.get('millions', {}).get('profiles', [])), 'smoke requires complete success'
            (work / 'SMOKE.json').write_text(json.dumps(pending, indent=2) + '\n')
            print('Smoke passed: both full-suite filesystems and all three million-suite engines; published results unchanged.')
        else:
            for name, value in pending.items():
                publish(name, value)
            command([sys.executable, str(HERE / 'report.py'), args.suite], work / 'render.log', env)
            print('Reports and SVG graphs regenerated in benchmark/.', flush=True)
        success = True
    finally:
        failed_profiles = any(p['report'].get('status') == 'failed'
                              for p in pending.get('millions', {}).get('profiles', []))
        if success and not args.keep_work and not failed_profiles:
            shutil.rmtree(work)
            shutil.rmtree(ram)
        else:
            print(f'Work retained: {work}\nRAM work retained: {ram}', file=sys.stderr)


if __name__ == '__main__':
    main()
