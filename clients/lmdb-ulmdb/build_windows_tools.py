#!/usr/bin/env python3
"""Cross/native MinGW build of the Windows migration and launcher pipeline.

Pinned LMDB readers are linked into separate exporter EXEs. Neither exporter
imports the adapter DLL. The importer loads only its explicitly selected DLL.
No Windows runtime result follows from this compile/link command.
"""
import argparse
import hashlib
import json
import os
from pathlib import Path
import shutil
import signal
import subprocess


def digest(path):
    h = hashlib.sha256()
    with path.open('rb') as f:
        for chunk in iter(lambda: f.read(1024 * 1024), b''):
            h.update(chunk)
    return h.hexdigest()


def package_notices(here, output, sources, inputs):
    """Retain the same pinned LMDB notices/source as the native packager."""
    notice_path = here / 'exporter-notices.json'
    notices = json.loads(notice_path.read_text())
    if digest(notice_path) != inputs[str(notice_path)]:
        raise RuntimeError('exporter notice manifest drift')
    licenses = output / 'share/licenses'
    licenses.mkdir(parents=True)
    shutil.copyfile(notice_path, licenses / notice_path.name)
    for source, metadata, version in sources:
        notice = notices[version]
        if notice['commit'] != metadata['commit']:
            raise RuntimeError('exporter notice commit mismatch: ' + version)
        for entry in notice['files']:
            path = source / entry['path']
            if path.stat().st_size != entry['bytes'] or digest(path) != entry['sha256']:
                raise RuntimeError('exporter notice mismatch: ' + str(path))
            if digest(path) != inputs[str(path)]:
                raise RuntimeError('exporter notice source drift: ' + str(path))
        target = licenses / ('lmdb-' + version)
        retained = target / 'source'
        retained.mkdir(parents=True)
        names = ('LICENSE', 'COPYRIGHT', 'mdb.c', 'midl.c', 'lmdb.h', 'midl.h')
        pins = {entry['path']: entry for entry in metadata['files']}
        for name in names:
            destination = (target if name in ('LICENSE', 'COPYRIGHT') else retained) / name
            shutil.copyfile(source / name, destination)
            if digest(destination) != pins[name]['sha256']:
                raise RuntimeError('copied exporter source mismatch: ' + str(destination))
        record = {key: metadata[key] for key in ('repository', 'ref', 'commit', 'license')}
        record['files'] = [pins[name] for name in names]
        (target / 'SOURCE.json').write_text(json.dumps(record, indent=2) + '\n')


def main():
    p = argparse.ArgumentParser(description=__doc__)
    p.add_argument('--modern-source', type=Path, required=True)
    p.add_argument('--legacy-source', type=Path, required=True)
    p.add_argument('--dll', type=Path, required=True)
    p.add_argument('--import-library', type=Path, help='also build ulmdb-tool.exe against this adapter import library')
    p.add_argument('--output', type=Path, required=True)
    p.add_argument('--cc', default='x86_64-w64-mingw32-gcc')
    p.add_argument('--objdump', default='x86_64-w64-mingw32-objdump')
    a = p.parse_args()
    here = Path(__file__).resolve().parent
    include = here.parent / 'c/include'
    out = a.output.resolve()
    out.mkdir(parents=True, exist_ok=False)
    bindir = out / 'bin'
    bindir.mkdir()
    receipt = dict(complete=False, passed=False, commands=[], scope='Windows source/link; no runtime validation')
    inputs = {str(f): digest(f) for f in [*here.glob('*.c'), *here.glob('*.h'),
              here / 'modern-source.json', here / 'legacy-source.json',
              here / 'exporter-notices.json', include / 'lmdb.h',
              Path(__file__).resolve(), a.dll.resolve()]}
    if a.import_library:
        inputs[str(a.import_library.resolve())] = digest(a.import_library.resolve())
    def run(argv, label):
        kwargs = dict(stdout=subprocess.PIPE, stderr=subprocess.PIPE)
        kwargs.update(creationflags=subprocess.CREATE_NEW_PROCESS_GROUP) if os.name == 'nt' else kwargs.update(start_new_session=True)
        proc = subprocess.Popen([str(x) for x in argv], **kwargs)
        row = dict(argv=[str(x) for x in argv], pid=proc.pid, terminal=False)
        receipt['commands'].append(row)
        try:
            stdout, stderr = proc.communicate(timeout=120)
        except BaseException:
            if os.name == 'nt':
                subprocess.run(['taskkill', '/PID', str(proc.pid), '/T', '/F'], stdout=subprocess.DEVNULL, stderr=subprocess.DEVNULL)
            else:
                os.killpg(proc.pid, signal.SIGKILL)
            stdout, stderr = proc.communicate(timeout=10)
            raise
        finally:
            if 'stdout' in locals():
                (out / (label + '.stdout')).write_bytes(stdout)
                (out / (label + '.stderr')).write_bytes(stderr)
            row.update(terminal=proc.poll() is not None, exit_code=proc.returncode)
        if proc.returncode:
            raise RuntimeError(label + ' failed')
        return stdout
    try:
        sources = []
        for source, pin, version in ((a.modern_source.resolve(), 'modern-source.json', '1.0.2'),
                                     (a.legacy_source.resolve(), 'legacy-source.json', '0.9.31')):
            metadata = json.loads((here / pin).read_text())
            for entry in metadata['files']:
                path = source / entry['path']
                actual = digest(path)
                if path.stat().st_size != entry['bytes'] or actual != entry['sha256']:
                    raise RuntimeError('pinned source mismatch: ' + str(path))
                inputs[str(path)] = actual
            sources.append((source, metadata, version))
        package_notices(here, out, sources, inputs)
        common = [a.cc, '-std=c11', '-O2', '-D_GNU_SOURCE', '-municode']
        strict = [*common, '-Wall', '-Wextra', '-Werror', '-I' + str(include)]
        for source, name in ((a.modern_source, 'lmdb-export.exe'),
                             (a.legacy_source, 'lmdb-export-0.9.exe')):
            run([*common, '-I' + str(source.resolve()), here / 'lmdb_export.c',
                 source.resolve() / 'mdb.c', source.resolve() / 'midl.c', '-o', bindir / name], name + '-link')
        units = ['migrate.c', 'migrate_stream.c', 'migrate_fs.c', 'operator_fs.c',
                 'operator_windows.c', 'windows_process.c']
        libs = ['-ladvapi32', '-lbcrypt', '-lntdll']
        run([*strict, *(here / n for n in units), *libs, '-o', bindir / 'ulmdb-migrate.exe'], 'migrator-link')
        run([*strict, here / 'convert-lmdb-to-ulmdb.c', here / 'operator_windows.c',
             here / 'windows_process.c', *libs, '-o', bindir / 'convert-lmdb-to-ulmdb.exe'], 'launcher-link')
        if a.import_library:
            run([*strict, *(here / n for n in ('operator.c', 'operator_stream.c',
                 'operator_fs.c', 'operator_inspect.c', 'operator_windows.c')),
                 a.import_library.resolve(), *libs, '-o', bindir / 'ulmdb-tool.exe'], 'operator-link')
        shutil.copyfile(a.dll, bindir / 'ulmdb_c.dll')
        # These are genuine provider aliases, never POSIX forwarding shims.
        for alias in ('lmdb.dll', 'liblmdb.dll', 'libulmdb_c.dll'):
            shutil.copyfile(a.dll, bindir / alias)
        imports = {}
        for exe in sorted(bindir.glob('*.exe')):
            output = run([a.objdump, '-p', exe], exe.name + '-imports').decode()
            names = [line.split('DLL Name:', 1)[1].strip() for line in output.splitlines() if 'DLL Name:' in line]
            imports[exe.name] = names
            if exe.name == 'ulmdb-tool.exe':
                if 'ulmdb_c.dll' not in [n.lower() for n in names]:
                    raise RuntimeError('operator import library does not select packaged ulmdb_c.dll')
            elif any('lmdb' in n.lower() for n in names):
                raise RuntimeError('exporter/importer/launcher must not import a competing MDB provider: ' + str(exe))
        after = {name: digest(Path(name)) for name in inputs}
        if after != inputs:
            raise RuntimeError('input drift')
        receipt.update(complete=True, passed=True, inputs=inputs, inputs_after=after, imports=imports,
                       outputs={str(f.relative_to(out)): digest(f)
                                for f in sorted(out.rglob('*')) if f.is_file()})
    except BaseException as error:
        receipt.update(error=repr(error), inputs=inputs)
        raise
    finally:
        (out / 'BUILD.json').write_text(json.dumps(receipt, indent=2) + '\n')


if __name__ == '__main__':
    main()
