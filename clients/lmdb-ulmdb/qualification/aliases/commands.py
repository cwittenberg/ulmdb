"""Preserve actual command evidence on deadlines, signals and startup failures."""
import hashlib
from pathlib import Path
import re
import subprocess
import time

import processes


def digest(path):
    with Path(path).open('rb') as stream:
        return hashlib.file_digest(stream, 'sha256').hexdigest()


def run(owner, name, argv, env=None, timeout=20, expected=0):
    """Run one owned command; a harness failure is always raised after logging.

    The owner supplies output, receipt['commands'] and save(). Only a process
    created by this call is stopped here; other live actor sessions stay owned
    by the caller's outer cleanup.
    """
    folder = owner.output / 'commands'
    folder.mkdir(exist_ok=True)
    label = re.sub(r'[^A-Za-z0-9_.-]', '_', name)
    stem = folder / (str(len(owner.receipt['commands'])) + '-' + label)
    stdout_path, stderr_path = Path(str(stem) + '.stdout'), Path(str(stem) + '.stderr')
    command = list(map(str, argv))
    before = set(processes.ACTIVE)
    process = None
    stdout, stderr = b'', b''
    failure = cleanup_failure = None
    started = time.monotonic()
    try:
        process = processes.start(command, env=env, stdout=subprocess.PIPE, stderr=subprocess.PIPE)
        stdout, stderr = process.communicate(timeout=timeout)
    except BaseException as error:
        failure = error
        # start() defers signals until its new process is registered. The signal
        # can therefore raise before assignment of its return value here.
        if process is None:
            created = [item for pid, item in processes.ACTIVE.items() if pid not in before]
            if len(created) == 1:
                process = created[0]
        if process is not None:
            try:
                stdout, stderr = processes.stop(process)
            except BaseException as cleanup_error:
                cleanup_failure = cleanup_error
                stdout = getattr(cleanup_error, 'stdout', b'') or getattr(error, 'output', b'') or b''
                stderr = getattr(cleanup_error, 'stderr', b'') or getattr(error, 'stderr', b'') or b''
    finally:
        # Record even Popen failure and partial output from a killed command.
        # Exception paths must never disappear from the command inventory.
        stdout_path.write_bytes(stdout or b'')
        stderr_path.write_bytes(stderr or b'')
        row = {'label': name, 'argv': command, 'pid': process.pid if process else None,
               'returncode': process.returncode if process else None,
               'deadline': isinstance(failure, (subprocess.TimeoutExpired, TimeoutError)),
               'interrupted': isinstance(failure, KeyboardInterrupt),
               'seconds_observed_not_performance': time.monotonic() - started,
               'stdout': str(stdout_path.relative_to(owner.output)),
               'stderr': str(stderr_path.relative_to(owner.output)),
               'stdout_sha256': digest(stdout_path), 'stderr_sha256': digest(stderr_path)}
        if failure is not None:
            row['harness_failure'] = {'type': type(failure).__name__, 'message': str(failure)}
        if cleanup_failure is not None:
            row['cleanup_failure'] = {'type': type(cleanup_failure).__name__, 'message': str(cleanup_failure)}
        if process is not None and failure is None and expected is not None and process.returncode != expected:
            row['unexpected_returncode'] = {'expected': expected, 'actual': process.returncode}
        owner.receipt['commands'].append(row)
        try:
            owner.save()
        finally:
            if process is not None and process.returncode is not None and cleanup_failure is None:
                processes.forget(process)
    if failure is not None:
        raise failure
    if expected is not None and process.returncode != expected:
        raise RuntimeError(str(row) + ': ' + stderr.decode(errors='replace'))
    return stdout, stderr, process.returncode
