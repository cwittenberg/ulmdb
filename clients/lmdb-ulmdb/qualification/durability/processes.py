"""Owned process-group cleanup for the Linux qualification tools."""
from contextlib import contextmanager
import os
from pathlib import Path
import signal
import subprocess
import json

ACTIVE = {}


class StageInterrupted(KeyboardInterrupt):
    pass


def install_handlers():
    def interrupted(signum, frame):
        raise StageInterrupted('qualification interrupted by signal ' + str(signum))
    for signum in [signal.SIGINT, signal.SIGTERM]:
        signal.signal(signum, interrupted)


@contextmanager
def ignore_interrupts():
    previous = {signum: signal.signal(signum, signal.SIG_IGN) for signum in [signal.SIGINT, signal.SIGTERM]}
    try:
        yield
    finally:
        for signum, handler in previous.items():
            signal.signal(signum, handler)


def stop(process):
    """Kill the group even when its leader exited but descendants own pipes."""
    with ignore_interrupts():
        try:
            os.killpg(process.pid, signal.SIGTERM)
        except ProcessLookupError:
            pass
        try:
            result = process.communicate(timeout=1)
            return result
        except subprocess.TimeoutExpired:
            try:
                os.killpg(process.pid, signal.SIGKILL)
            except ProcessLookupError:
                pass
            try:
                result = process.communicate(timeout=2)
                return result
            except subprocess.TimeoutExpired as exc:
                # No unbounded wait is permitted even on an unkillable host IO
                # task. Callers retain these bytes and fail qualification.
                error = RuntimeError('owned group did not terminate after SIGKILL')
                error.stdout = exc.output or b''
                error.stderr = exc.stderr or b''
                raise error from exc


def start(argv, **kwargs):
    # A signal during Popen must not lose the newly created process before its
    # caller receives the handle. Defer raising while registering it; do not
    # block OS signals that the exec'd child would otherwise inherit blocked.
    pending = []
    def defer(signum, frame):
        pending.append(signum)
    previous = {signum: signal.signal(signum, defer) for signum in [signal.SIGINT, signal.SIGTERM]}
    try:
        process = subprocess.Popen(argv, start_new_session=True, **kwargs)
        ACTIVE[process.pid] = process
    finally:
        for signum, handler in previous.items():
            signal.signal(signum, handler)
    if pending:
        raise StageInterrupted('qualification interrupted during process creation: ' + str(pending[-1]))
    return process


def forget(process):
    ACTIVE.pop(process.pid, None)


def cleanup_all(directory):
    """Fallback for interruption during process creation or result recording."""
    directory = Path(directory)
    with ignore_interrupts():
        for process in list(ACTIVE.values()):
            error = None
            try:
                stdout, stderr = stop(process)
            except BaseException as exc:
                error = {'type': type(exc).__name__, 'message': str(exc)}
                stdout, stderr = getattr(exc, 'stdout', b''), getattr(exc, 'stderr', b'')
            prefix = directory / ('owned-cleanup-' + str(process.pid))
            prefix.with_suffix('.stdout').write_bytes(getattr(process, '_owned_stdout_prefix', b'') + (stdout or b''))
            prefix.with_suffix('.stderr').write_bytes(stderr or b'')
            prefix.with_suffix('.json').write_text(json.dumps({'argv': process.args, 'pid': process.pid,
                'returncode': process.returncode, 'error': error}, indent=2) + '\n')
            if error is None:
                forget(process)
