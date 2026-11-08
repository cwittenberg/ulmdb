#!/usr/bin/env python3
"""Explicitly build the empty-mode helper, without writing repository sources."""
from __future__ import annotations

import argparse
import hashlib
import json
import os
from pathlib import Path
import shutil
import signal
import subprocess
import sys
import tomllib
import types

HERE = Path(__file__).resolve().parent
OWNED = types.ModuleType('durability_helper_owned_processes')
OWNED.__file__ = str(HERE / 'processes.py')
exec(compile((HERE / 'processes.py').read_bytes(), str(HERE / 'processes.py'), 'exec'), OWNED.__dict__)
ENTRY_HASH = 'd3122fa934234dfff338080293fb81d4aa837704e28b108c9f8f41238a45c7e2'


def digest(path: Path) -> str:
    with path.open('rb') as stream:
        return hashlib.file_digest(stream, 'sha256').hexdigest()


def native_sources(root: Path) -> list[Path]:
    paths = [root / name for name in ['Cargo.toml', 'Cargo.lock', 'rust-toolchain.toml']]
    config = root / '.cargo/config.toml'
    if config.is_file():
        paths.append(config)
    for name in ['ulmdb', 'ulmdb-core', 'ulmdb-storage', 'ulmdb-kv', 'ulmdb-prefix']:
        directory = root / 'crates' / name
        if not directory.is_dir():
            raise RuntimeError(f'missing matched Native source: {directory}')
        paths += [p for p in directory.rglob('*') if p.is_file() and p.suffix in {'.rs', '.toml', '.c', '.h'}]
    if not all(p.is_file() and not p.is_symlink() for p in paths):
        raise RuntimeError('Native source manifest requires regular source files')
    return sorted(set(paths))


def main() -> int:
    ap = argparse.ArgumentParser(description=__doc__)
    ap.add_argument('--source-root', type=Path, required=True)
    ap.add_argument('--output', type=Path, required=True, help='new directory; owns source copy, lock, target and provenance')
    ap.add_argument('--timeout', type=float, default=600)
    a = ap.parse_args()
    OWNED.install_handlers()
    root, out = a.source_root.resolve(), a.output.resolve()
    out.mkdir(parents=True, exist_ok=False)
    receipt = {'schema': 1, 'role': 'empty-native-backend-initializer', 'complete': False, 'passed': False,
               'source_root': str(root), 'commands': [], 'sources': [], 'product_qualified': False}
    save = lambda: (out / 'provenance.json').write_text(json.dumps(receipt, indent=2) + '\n')
    save()
    env = dict(os.environ)
    for name in ['RUSTC_WRAPPER', 'RUSTC_WORKSPACE_WRAPPER', 'CARGO_BUILD_TARGET']:
        env.pop(name, None)
    env['CARGO_TARGET_DIR'] = str(out / 'target')
    def command(label: str, argv: list[str], cwd: Path) -> None:
        process = OWNED.start(argv, cwd=cwd, env=env, stdout=subprocess.PIPE, stderr=subprocess.PIPE)
        try:
            stdout, stderr = process.communicate(timeout=a.timeout)
        except BaseException as error:
            with OWNED.ignore_interrupts():
                try:
                    stdout, stderr = OWNED.stop(process)
                except BaseException as cleanup:
                    stdout, stderr = getattr(cleanup, 'stdout', b''), getattr(cleanup, 'stderr', b'')
                (out / (label + '.stdout')).write_bytes(stdout or b'')
                (out / (label + '.stderr')).write_bytes(stderr or b'')
                receipt['commands'].append({'label': label, 'argv': argv, 'returncode': process.returncode,
                    'timeout': isinstance(error, subprocess.TimeoutExpired), 'interrupted': isinstance(error, KeyboardInterrupt),
                    'error': {'type': type(error).__name__, 'message': str(error)},
                    'stdout_sha256': digest(out / (label + '.stdout')), 'stderr_sha256': digest(out / (label + '.stderr'))})
                save()
                OWNED.forget(process)
            raise RuntimeError(label + ': owned process deadline/interruption') from error
        (out / (label + '.stdout')).write_bytes(stdout)
        (out / (label + '.stderr')).write_bytes(stderr)
        receipt['commands'].append({'label': label, 'argv': argv, 'cwd': str(cwd), 'returncode': process.returncode,
                                    'stdout_sha256': digest(out / (label + '.stdout')),
                                    'stderr_sha256': digest(out / (label + '.stderr'))})
        save()
        OWNED.forget(process)
        if process.returncode:
            raise RuntimeError(f'{label}: failed; inspect {label}.stderr')
    try:
        if sys.platform != 'linux' or not shutil.which('cargo'):
            raise RuntimeError('requires Linux and an already installed Cargo/Rust toolchain')
        if digest(HERE / 'backend_init/main.rs') != ENTRY_HASH:
            raise RuntimeError('empty initializer source differs from frozen helper')
        source = native_sources(root) + [HERE / 'backend_init/main.rs', Path(__file__).resolve(), HERE / 'processes.py']
        before = {str(p): digest(p) for p in source}
        receipt['sources'] = [{'path': p, 'sha256': h} for p, h in before.items()]
        receipt['entry_source_sha256'] = ENTRY_HASH
        if shutil.which('rustup'):
            channel = env.get('RUSTUP_TOOLCHAIN') or tomllib.loads((root / 'rust-toolchain.toml').read_text())['toolchain']['channel']
            command('installed-toolchains', ['rustup', 'toolchain', 'list'], root)
            installed = (out / 'installed-toolchains.stdout').read_text()
            if not any(line.split()[0] == channel or line.split()[0].startswith(channel + '-')
                       for line in installed.splitlines() if line.split()):
                raise RuntimeError(f'install requested Rust toolchain explicitly before running: {channel}')
        command('cargo-version', ['cargo', '--version'], root)
        command('rustc-version', ['rustc', '-vV'], root)
        project = out / 'project'
        (project / 'src').mkdir(parents=True)
        shutil.copyfile(HERE / 'backend_init/main.rs', project / 'src/main.rs')
        (project / 'Cargo.toml').write_text('[package]\nname="c-policy-empty-backend-init"\nversion="0.0.0"\nedition="2024"\n\n[workspace]\n\n[dependencies]\nulmdb={path=' + json.dumps(str(root / 'crates/ulmdb')) + '}\n')
        shutil.copyfile(root / 'Cargo.lock', project / 'Cargo.lock')
        if (out / 'target').exists():
            raise RuntimeError('helper target must start absent')
        receipt['target_initially_absent'] = True
        # Cargo may add the helper package to its OUTPUT lockfile. Repository
        # sources/lock remain untouched; --offline prevents dependency downloads.
        command('build', ['cargo', 'build', '--offline', '--manifest-path', str(project / 'Cargo.toml')], root)
        executable = out / 'target/debug/c-policy-empty-backend-init'
        if not executable.is_file():
            raise RuntimeError('helper build produced no host executable')
        if any(digest(Path(p)) != h for p, h in before.items()):
            raise RuntimeError('Native/helper sources changed during build')
        if set(native_sources(root)) != set(source) - {HERE / 'backend_init/main.rs', Path(__file__).resolve(), HERE / 'processes.py'}:
            raise RuntimeError('Native source inventory changed during build')
        if OWNED.ACTIVE:
            raise RuntimeError('owned process registry is not empty after helper build')
        receipt.update(complete=True, passed=True, executable=str(executable), executable_sha256=digest(executable),
                       generated_sources={str(p.relative_to(out)): digest(p) for p in project.rglob('*') if p.is_file()},
                       source_before_after_equal=True,
                       scope='empty physical mode initialization only; no C provider qualification inherited')
    except BaseException as exc:
        with OWNED.ignore_interrupts():
            OWNED.cleanup_all(out)
            receipt.update(complete=False, passed=False, error={'type': type(exc).__name__, 'message': str(exc)})
            save()
        raise
    finally:
        with OWNED.ignore_interrupts():
            OWNED.cleanup_all(out)
            save()
    print(out / 'provenance.json')
    return 0


if __name__ == '__main__':
    raise SystemExit(main())
