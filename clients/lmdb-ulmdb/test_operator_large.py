#!/usr/bin/env python3
"""Bounded 129MiB public operator pipe, spooling and atomic-input regressions."""
import argparse
import hashlib
import json
import os
from pathlib import Path
import resource
import signal
import stat
import subprocess
import sys
import tempfile
import time

MIB = 1024 * 1024
VALUE_BYTES = 129 * MIB
BLOCK = bytes(range(256)) * 256
HEX = BLOCK.hex().encode()
HEADER = b"ULMDB-DUMP=1\npagesize=4096\nmapsize=1073741824\nHEADER=END\nDB=00000000:\n"


def emit(kind):
    out = sys.stdout.buffer
    out.write(HEADER + b" 626967\n ")
    for _ in range(VALUE_BYTES // len(BLOCK)):
        out.write(HEX)
    if kind == "badhex":
        out.write(b"g0\n")
        return
    out.write(b"\nDATA=END\n")
    if kind == "missing-footer":
        return
    if kind == "badflags":
        out.write(b"DB=80000000:78\nDATA=END\n")
    out.write(b"DUMP=END\n")
    if kind == "tail":
        out.write(b"trailing\n")


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--package", type=Path, required=True)
    parser.add_argument("--output", type=Path)
    parser.add_argument("--catalog-count", type=int, default=8193)
    args = parser.parse_args()
    def interrupted(signum, frame):
        raise SystemExit(128 + signum)
    signal.signal(signal.SIGTERM, interrupted)
    assert 0 <= args.catalog_count <= 65528
    package = args.package.resolve()
    tool = package / "bin/ulmdb-tool"
    owned = tempfile.TemporaryDirectory(prefix="ulmdb-operator-large-") if not args.output else None
    root = Path(owned.name) if owned else args.output.resolve()
    if not owned:
        root.mkdir(parents=True, exist_ok=False)
    spool = root / "private-spool"
    spool.mkdir(mode=0o700)
    env = dict(os.environ, LD_LIBRARY_PATH=str(package / "lib"),
               ULMDB_LMDB_ABI="1.0", TMPDIR=str(spool))
    rows, live, seen_spools = [], {}, set()

    def memory_bound():
        # Bounds heap/private data, not the genuine shared file-backed writer
        # VMA. A full 129MiB heap field cannot fit under this limit.
        resource.setrlimit(resource.RLIMIT_DATA, (96 * MIB, 96 * MIB))
        resource.setrlimit(resource.RLIMIT_CORE, (0, 0))

    def start(name, argv, stdin=None, pipe=False, data=None):
        out = (root / (name + ".stdout")).open("wb")
        err = (root / (name + ".stderr")).open("wb")
        argv = [str(x) for x in argv]
        # Register the owned process before a pending interrupt can unwind.
        # The child restores the caller mask before exec; no application locks
        # or threads exist in this focused controller's preexec function.
        mask = signal.pthread_sigmask(signal.SIG_BLOCK, {signal.SIGINT, signal.SIGTERM})
        def child_setup():
            signal.pthread_sigmask(signal.SIG_SETMASK, mask)
            memory_bound()
        try:
            process = subprocess.Popen(argv, stdin=subprocess.PIPE if data is not None else stdin,
                                       stdout=subprocess.PIPE if pipe else out, stderr=err,
                                       env=env, start_new_session=True, preexec_fn=child_setup)
            live[process.pid] = (process, name, argv, out, err)
        finally:
            signal.pthread_sigmask(signal.SIG_SETMASK, mask)
        if data is not None:
            process.stdin.write(data)
            process.stdin.close()
            process.stdin = None
        return process

    def inspect_spool(process):
        directory = Path(f"/proc/{process.pid}/fd")
        try:
            for fd in directory.iterdir():
                try:
                    target = os.readlink(fd)
                    if target.startswith(str(spool) + "/ulmdb-tool-field-"):
                        metadata = fd.stat()
                        assert stat.S_ISREG(metadata.st_mode)
                        assert stat.S_IMODE(metadata.st_mode) == 0o600
                        assert metadata.st_nlink == 0 and target.endswith(" (deleted)")
                        flags = next(x for x in Path(f"/proc/{process.pid}/fdinfo/{fd.name}").read_text().splitlines()
                                     if x.startswith("flags:")).split()[1]
                        assert int(flags, 8) & os.O_CLOEXEC
                        seen_spools.add(target)
                except (FileNotFoundError, ProcessLookupError, PermissionError):
                    pass
        except (FileNotFoundError, ProcessLookupError, PermissionError):
            pass

    def finish(process, good=True, timeout=90, acceptable=None):
        begin = time.monotonic()
        while True:
            inspect_spool(process)
            pid, status, usage = os.wait4(process.pid, os.WNOHANG)
            if pid:
                process.returncode = os.waitstatus_to_exitcode(status)
                _, name, argv, out, err = live.pop(pid)
                out.close()
                err.close()
                row = dict(name=name, argv=argv, returncode=process.returncode,
                           max_rss_kib=usage.ru_maxrss, heap_limit_bytes=96 * MIB)
                rows.append(row)
                assert ((process.returncode in acceptable) if acceptable is not None else
                        (process.returncode == 0) == good), (row, (root / (name + ".stderr")).read_bytes())
                assert usage.ru_maxrss <= 384 * 1024, row
                return
            assert time.monotonic() - begin < timeout, ("owned command deadline", process.pid)
            time.sleep(0.005)

    def simple(name, argv, data=None, good=True):
        process = start(name, argv, data=data)
        finish(process, good)
        return (root / (name + ".stdout")).read_bytes()

    def load_stream(name, destination, kind="valid", good=True, options=()):
        producer = start(name + "-producer", [sys.executable, __file__, "--emit", kind], pipe=True)
        consumer = start(name + "-load", [tool, "load", *options, destination], stdin=producer.stdout)
        producer.stdout.close()
        finish(consumer, good)
        # A parser may reject immediately after the bad byte/flags. Its producer
        # can then observe EPIPE; it must still be owned, stopped and recorded.
        finish(producer, acceptable={0} if good else {0, 1})
        assert not (root / (name + "-producer.stderr")).read_bytes()

    try:
        source, destination, existing = root / "source", root / "copy", root / "existing"
        load_stream("seed", source)
        producer = start("pipe-dump", [tool, "dump", source], pipe=True)
        consumer = start("pipe-load", [tool, "load", destination], stdin=producer.stdout)
        producer.stdout.close()
        finish(consumer)
        finish(producer)
        # Hash tool get through another actual pipe. No complete result is held
        # by the test or operator, and both cold reopen and exact bytes matter.
        expected = hashlib.sha256()
        for _ in range(VALUE_BYTES // len(BLOCK)):
            expected.update(HEX)
        expected.update(b"\n")
        producer = start("verify-get", [tool, "get", destination, "626967"], pipe=True)
        consumer = start("verify-hash", [sys.executable, __file__, "--hash"], stdin=producer.stdout)
        producer.stdout.close()
        finish(consumer)
        finish(producer)
        assert (root / "verify-hash.stdout").read_text().strip() == expected.hexdigest()
        small = HEADER + b" 6b\n 6f6c64\nDATA=END\nDUMP=END\n"
        simple("existing-seed", [tool, "load", existing], data=small)
        before = (existing / "CURRENT").read_bytes()
        for kind in ("badhex", "missing-footer", "badflags", "tail"):
            load_stream(kind, existing, kind, good=False)
            assert (existing / "CURRENT").read_bytes() == before
            assert simple(kind + "-old-value", [tool, "get", existing, "6b"]) == b"6f6c64\n"
            assert not list(spool.iterdir())
        fresh = root / "malformed-new"
        load_stream("fresh-badhex", fresh, "badhex", good=False)
        assert not fresh.exists() and not list(root.glob("malformed-new.load-*"))
        small_map = root / "too-small-map"
        load_stream("too-small-map", small_map, good=False, options=("--mapsize", "16777216"))
        assert not small_map.exists() and not list(root.glob("too-small-map.load-*"))
        # Decimal overflow/zero/signs must reject before creating a target.
        for index, value in enumerate((b"0", b"-1", b"18446744073709551616")):
            bad = small.replace(b"1073741824", value)
            target = root / f"bad-map-{index}"
            simple(f"bad-map-{index}", [tool, "load", target], data=bad, good=False)
            assert not target.exists() and not list(root.glob(target.name + ".load-*"))
        repeated = HEADER + b"DATA=END\nDB=00000000:78\nDATA=END\nDB=00000000:78\nDATA=END\nDUMP=END\n"
        repeated_target = root / "repeated-section"
        simple("repeated-section", [tool, "load", repeated_target], data=repeated, good=False)
        assert not repeated_target.exists() and not list(root.glob("repeated-section.load-*"))
        if args.catalog_count:
            # Empty user databases exceed the old operator's 8192-slot ceiling.
            # Metadata is bounded by configured catalog capacity, never values.
            target = root / "catalog"
            stream = root / "catalog.dump"
            with stream.open("wb") as out:
                out.write(HEADER + b"DATA=END\n")
                for n in range(args.catalog_count):
                    out.write(b"DB=00000000:" + f"db{n:05d}".encode().hex().encode() + b"\nDATA=END\n")
                out.write(b"DUMP=END\n")
            with stream.open("rb") as incoming:
                finish(start("catalog-load", [tool, "load", target], stdin=incoming))
            report = simple("catalog-stat", [tool, "stat", "--json", target])
            assert len(json.loads(report)["databases"]) == args.catalog_count + 1
        assert seen_spools, "no actual private unlinked spool was observed"
        assert not list(spool.iterdir())
        receipt = dict(status="pass", value_bytes=VALUE_BYTES, catalog_count=args.catalog_count,
                       commands=rows, observed_private_spools=sorted(seen_spools),
                       tool_sha256=hashlib.file_digest(tool.open("rb"), "sha256").hexdigest(),
                       provider_sha256=hashlib.file_digest((package / "lib/liblmdb.so.0").open("rb"), "sha256").hexdigest())
        (root / "receipt.json").write_text(json.dumps(receipt, indent=2) + "\n")
        print("operator129MiB pipe/private-spool/96MiB-heap/atomic-input/catalog: PASS")
    except BaseException as error:
        # Preserve failed commands as well as completed commands. Each PID is
        # still our unreaped child; kill its own group before wait releases it.
        for pid, (process, name, argv, out, err) in list(live.items()):
            try:
                os.killpg(pid, signal.SIGKILL)
            except ProcessLookupError:
                pass
            _, status, usage = os.wait4(pid, 0)
            process.returncode = os.waitstatus_to_exitcode(status)
            out.close()
            err.close()
            rows.append(dict(name=name, argv=argv, returncode=process.returncode,
                             max_rss_kib=usage.ru_maxrss, interrupted=True,
                             heap_limit_bytes=96 * MIB))
            del live[pid]
        (root / "receipt.json").write_text(json.dumps(dict(status="fail", error=repr(error),
                                                         commands=rows), indent=2) + "\n")
        raise
    finally:
        # Normal completion has no remaining children. This still covers an
        # exception raised while writing the final success/failure receipt.
        for process, _, _, out, err in live.values():
            if process.returncode is None:
                try:
                    os.killpg(process.pid, signal.SIGKILL)
                except ProcessLookupError:
                    pass
                process.wait()
            out.close()
            err.close()
        if owned:
            owned.cleanup()


if __name__ == "__main__":
    if len(sys.argv) == 3 and sys.argv[1] == "--emit":
        try:
            emit(sys.argv[2])
            sys.stdout.buffer.flush()
        except BrokenPipeError:
            # Explicit error for parent accounting, without traceback noise.
            os._exit(1)
    elif len(sys.argv) == 2 and sys.argv[1] == "--hash":
        digest = hashlib.sha256()
        while block := sys.stdin.buffer.read(65536):
            digest.update(block)
        print(digest.hexdigest())
    else:
        main()
