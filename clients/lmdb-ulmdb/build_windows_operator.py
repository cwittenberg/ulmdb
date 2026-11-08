#!/usr/bin/env python3
"""Build the Windows public-C operator against an explicit DLL/import library.

Run on a MinGW host or cross toolchain. This does not execute or qualify Windows.
The linked provider and all source bytes are bound before/after in BUILD.json.
"""
import argparse
import hashlib
import json
import os
from pathlib import Path
import shutil
import signal
import subprocess

SOURCES = ('operator.c', 'operator_stream.c', 'operator_fs.c', 'operator_inspect.c',
           'operator_windows.c', 'operator_windows.h', 'operator_fs_windows.c', 'operator.h')


def sha(path):
    h = hashlib.sha256()
    with path.open('rb') as f:
        for block in iter(lambda: f.read(1024 * 1024), b''):
            h.update(block)
    return h.hexdigest()


def main():
    ap = argparse.ArgumentParser(description=__doc__)
    ap.add_argument('--dll', type=Path, required=True)
    ap.add_argument('--import-library', type=Path, required=True)
    ap.add_argument('--output', type=Path, required=True)
    ap.add_argument('--cc', default=os.environ.get('CC', 'x86_64-w64-mingw32-gcc'))
    ap.add_argument('--objdump', default='x86_64-w64-mingw32-objdump')
    ap.add_argument('--vectors', action='store_true')
    args = ap.parse_args()
    here = Path(__file__).resolve().parent
    include = here.parent / 'c/include'
    output = args.output.resolve()
    output.mkdir(parents=True, exist_ok=False)
    binpath = output / 'bin'
    binpath.mkdir()
    inputs = [here / name for name in SOURCES] + [include / 'lmdb.h',
              args.dll.resolve(), args.import_library.resolve()]
    if args.vectors:
        inputs += [here / 'tests/operator_windows_vectors.c']
    before = {str(p): sha(p) for p in inputs}
    receipt = dict(complete=False, passed=False, inputs=before, commands=[],
                   scope='compiled Windows x86_64 operator; not Windows runtime validation')
    def execute(command, label):
        kwargs = dict(stdout=subprocess.PIPE, stderr=subprocess.PIPE)
        if os.name == 'nt':
            kwargs['creationflags'] = subprocess.CREATE_NEW_PROCESS_GROUP
        else:
            kwargs['start_new_session'] = True
        process = subprocess.Popen(command, **kwargs)
        row = dict(command=command, pid=process.pid, terminal=False)
        receipt['commands'].append(row)
        try:
            out, err = process.communicate(timeout=120)
        except BaseException:
            if os.name == 'nt':
                subprocess.run(['taskkill', '/PID', str(process.pid), '/T', '/F'],
                               stdout=subprocess.DEVNULL, stderr=subprocess.DEVNULL)
            else:
                os.killpg(process.pid, signal.SIGKILL)
            out, err = process.communicate(timeout=10)
            raise
        finally:
            if 'out' in locals():
                (output / (label + '.stdout')).write_bytes(out)
                (output / (label + '.stderr')).write_bytes(err)
            row.update(terminal=process.poll() is not None, exit_code=process.returncode)
        if process.returncode:
            raise RuntimeError(f'{label}: compiler exit {process.returncode}')
        return out
    try:
        dll = binpath / args.dll.name
        shutil.copyfile(args.dll, dll)
        common = [args.cc, '-std=c11', '-O2', '-D_GNU_SOURCE', '-Wall', '-Wextra',
                  '-Werror', '-municode', '-I' + str(include)]
        units = [str(here / n) for n in SOURCES if n.endswith('.c') and n != 'operator_fs_windows.c']
        exe = binpath / 'ulmdb-tool.exe'
        execute(common + units + [str(args.import_library.resolve()), '-ladvapi32',
                '-lbcrypt', '-lntdll', '-o', str(exe)], 'link')
        imports = execute([args.objdump, '-p', str(exe)], 'imports').decode('utf-8')
        # Exact provider basename is selected by the import archive, not PATH guesses.
        names = [line.split('DLL Name:', 1)[1].strip() for line in imports.splitlines()
                 if 'DLL Name:' in line]
        if dll.name.lower() not in [name.lower() for name in names]:
            raise RuntimeError(f'import library does not select supplied DLL {dll.name}: {names}')
        receipt['imports'] = names
        if args.vectors:
            execute(common + ['-I' + str(here), str(here / 'tests/operator_windows_vectors.c'),
                    str(here / 'operator_windows.c'), str(here / 'operator_fs.c'),
                    '-ladvapi32', '-lbcrypt', '-lntdll', '-o', str(binpath / 'operator-windows-vectors.exe')], 'vectors-link')
        receipt['inputs_after'] = {str(p): sha(p) for p in inputs}
        if receipt['inputs_after'] != before or sha(dll) != before[str(args.dll.resolve())]:
            raise RuntimeError('input/provider bytes changed during build')
        receipt['outputs'] = {str(p.relative_to(output)): sha(p) for p in sorted(binpath.iterdir())}
        receipt.update(complete=True, passed=True)
    except BaseException as error:
        receipt['error'] = repr(error)
        raise
    finally:
        (output / 'BUILD.json').write_text(json.dumps(receipt, indent=2) + '\n')


if __name__ == '__main__':
    main()
