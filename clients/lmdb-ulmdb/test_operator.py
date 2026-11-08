#!/usr/bin/env python3
"""Focused packaged operator journeys; optional real upstream VERSION3 tools."""
import argparse
import json
import os
from pathlib import Path
import signal
import subprocess
import tempfile

PAYLOAD = b"""ULMDB-DUMP=1
pagesize=4096
mapsize=67108864
HEADER=END
DB=00000000:
 00ff80
 
 6b
 00ff0a
DATA=END
DB=00000002:726576
 6162
 4142
 6261
 4241
DATA=END
DB=00000008:696e74
 01000000
 31
 00010000
 323536
DATA=END
DB=00000004:647570
 6b
 0001
 6b
 00ff
DATA=END
DB=00000014:6669786564
 6b
 0001
 6b
 00ff
DATA=END
DB=00000034:64757073
 6b
 01000000
 6b
 00010000
DATA=END
DB=00000044:7264757073
 6b
 6162
 6b
 6261
DATA=END
DB=00000000:62696e0aff
 010a00
 ff000a
DATA=END
DUMP=END
"""


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--package", type=Path, required=True)
    parser.add_argument("--upstream-tools", type=Path)
    args = parser.parse_args()
    package = args.package.resolve()
    tool = package / "bin/ulmdb-tool"
    env = dict(os.environ, LD_LIBRARY_PATH=str(package / "lib"), ULMDB_LMDB_ABI="1.0")

    def command(argv, data=None, good=True):
        process = subprocess.Popen([str(x) for x in argv], stdin=subprocess.PIPE,
                                   stdout=subprocess.PIPE, stderr=subprocess.PIPE,
                                   env=env, start_new_session=True)
        try:
            out, error = process.communicate(data, timeout=15)
        except BaseException:
            os.killpg(process.pid, signal.SIGKILL)
            process.wait()
            raise
        assert (process.returncode == 0) == good, (argv, process.returncode, error)
        return out

    with tempfile.TemporaryDirectory(prefix="ulmdb-operator-") as temporary:
        root = Path(temporary)
        source, destination = root / "source", root / "pipe-target"
        command([tool, "load", source], PAYLOAD)
        expected = command([tool, "dump", source])
        # Real concurrent pipe; no complete dump is staged by the test/tool.
        producer = subprocess.Popen([str(tool), "dump", str(source)], stdout=subprocess.PIPE,
                                    stderr=subprocess.PIPE, env=env, start_new_session=True)
        consumer = None
        try:
            consumer = subprocess.Popen([str(tool), "load", str(destination)], stdin=producer.stdout,
                                        stdout=subprocess.PIPE, stderr=subprocess.PIPE,
                                        env=env, start_new_session=True)
            producer.stdout.close()
            out, error = consumer.communicate(timeout=15)
            producer.wait(timeout=15)
            assert consumer.returncode == producer.returncode == 0, error
        finally:
            for process in (consumer, producer):
                if process is not None and process.poll() is None:
                    os.killpg(process.pid, signal.SIGKILL)
                    process.wait()
        assert command([tool, "dump", destination]) == expected
        assert command([tool, "get", destination, "00ff80"]) == b"\n"
        assert command([tool, "get", "-s", os.fsdecode(b"bin\n\xff"), destination, "010a00"]) == b"ff000a\n"
        stat = command([tool, "stat", destination])
        for flag in (0, 2, 4, 8, 20, 52, 68):
            assert f" flags={flag} ".encode() in stat
        assert b"check=ok" in command([tool, "check", destination])
        machine = json.loads(command([tool, "stat", "--json", destination]))
        assert machine["schema"] == 1 and machine["transaction"] > 0
        assert machine["structural_check"] is False
        assert {entry["name_hex"] for entry in machine["databases"]} == {
            "", "rev".encode().hex(), "int".encode().hex(), "dup".encode().hex(),
            "fixed".encode().hex(), "dups".encode().hex(), "rdups".encode().hex(),
            b"bin\n\xff".hex(),
        }
        assert json.loads(command([tool, "check", "--json", destination]))["structural_check"] is True
        command([tool, "readers", destination])
        readers = json.loads(command([tool, "readers", "--clear", "--json", destination]))
        assert readers["cleanup_requested"] is True and readers["cleared"] >= 0
        assert all(isinstance(line, str) for line in readers["lines"])
        command([tool, "get", "--json", destination, "00ff80"], good=False)
        for suffix, extra in (("raw-copy", []), ("compact-copy", ["--compact"])):
            copy = root / suffix
            command([tool, "copy", *extra, destination, copy])
            assert command([tool, "dump", copy]) == expected
        # Incremental replay uses the exact physical checkpoint, not a logical
        # dump of the same rows. The stream works through an actual pipe.
        base = machine["transaction"]
        delta_target = root / "delta-target"
        command([tool, "copy", destination, delta_target])
        changed = PAYLOAD.replace(b" 00ff0a\n", b" 0102030405\n")
        command([tool, "load", destination], changed)
        producer = subprocess.Popen([str(tool), "delta-dump", str(destination), str(base)],
                                    stdout=subprocess.PIPE, stderr=subprocess.PIPE,
                                    env=env, start_new_session=True)
        consumer = None
        try:
            consumer = subprocess.Popen([str(tool), "delta-load", str(delta_target)],
                                        stdin=producer.stdout, stdout=subprocess.PIPE,
                                        stderr=subprocess.PIPE, env=env, start_new_session=True)
            producer.stdout.close()
            _, error = consumer.communicate(timeout=15)
            producer.wait(timeout=15)
            assert consumer.returncode == producer.returncode == 0, error
        finally:
            for process in (consumer, producer):
                if process is not None and process.poll() is None:
                    os.killpg(process.pid, signal.SIGKILL)
                    process.wait()
        assert command([tool, "dump", delta_target]) == command([tool, "dump", destination])
        delta = command([tool, "delta-dump", destination, str(base)])
        selected = (delta_target / "CURRENT").read_bytes()
        command([tool, "delta-load", delta_target], delta, good=False)  # replay
        command([tool, "delta-load", delta_target], delta[:-1], good=False)
        assert (delta_target / "CURRENT").read_bytes() == selected
        missing_base = root / "missing-incremental-base"
        command([tool, "delta-load", missing_base], delta, good=False)
        assert not missing_base.exists()
        command([tool, "delta-dump", destination, "0"], good=False)
        command([tool, "delta-dump", destination, "-1"], good=False)
        expected = command([tool, "dump", destination])
        before = (destination / "CURRENT").read_bytes()
        prefix = (b"ULMDB-DUMP=1\npagesize=4096\nmapsize=67108864\nHEADER=END\n"
                  b"DB=00000000:\n 6b\n 4348414e474544\n")
        malformed = [prefix + b" 0\n 61\nDATA=END\nDUMP=END\n",
                     prefix + b" 0102\nDATA=END\nDUMP=END\n",
                     prefix + b"DATA=END\n",  # missing whole-dump footer
                     prefix + b"DATA=END\nDUMP=END\ntrailing\n"]
        for data in malformed:
            command([tool, "load", destination], data, good=False)
            assert (destination / "CURRENT").read_bytes() == before
            assert command([tool, "dump", destination]) == expected
        fresh = root / "malformed-new"
        command([tool, "load", fresh], malformed[0], good=False)
        assert not fresh.exists() and not list(root.glob("malformed-new.load-*"))
        # An unsupported printable header name fails, never silently truncates.
        command([tool, "dump", "--lmdb", "-s", os.fsdecode(b"bin\n\xff"), source], good=False)
        if args.upstream_tools:
            upstream = args.upstream_tools.resolve()
            for name in ("rev", "int", "dup", "fixed", "dups", "rdups"):
                stream = command([tool, "dump", "--lmdb", "-s", name, source])
                lmdb_env = root / (name + "-lmdb")
                lmdb_env.mkdir()
                command([upstream / "mdb_load", lmdb_env], stream)
                original = command([upstream / "mdb_dump", "-s", name, lmdb_env])
                native = root / (name + "-native")
                command([tool, "load", native], original)
                assert command([tool, "dump", "--lmdb", "-s", name, native]) == stream
        print("operator pipe/binary/flags/empty/duplicates/atomic/copy/reopen: PASS")


if __name__ == "__main__":
    main()
