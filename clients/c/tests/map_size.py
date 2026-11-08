#!/usr/bin/env python3
"""Exercise published map-capacity adoption through the C API across processes."""
from __future__ import annotations

import ctypes as C
import json
import os
from pathlib import Path
import select
import subprocess
import sys
import tempfile
import unittest

ROOT = Path(__file__).resolve().parents[3]
LIBRARY = Path(os.environ.get("ULMDB_LIBRARY", ROOT / "target/debug/libulmdb_c.so")).resolve()
MIB = 1024 * 1024


class Value(C.Structure):
    _fields_ = [("size", C.c_size_t), ("data", C.c_void_p)]


class Info(C.Structure):
    _fields_ = [("address", C.c_void_p), ("mapsize", C.c_size_t),
                ("last_page", C.c_size_t), ("last_txn", C.c_size_t),
                ("max_readers", C.c_uint), ("num_readers", C.c_uint)]


def library():
    lib = C.CDLL(str(LIBRARY))
    handle = C.c_void_p
    for name, arguments in {
        "mdb_env_create": [C.POINTER(handle)],
        "mdb_env_set_mapsize": [handle, C.c_size_t],
        "mdb_env_open": [handle, C.c_char_p, C.c_uint, C.c_uint],
        "mdb_env_info": [handle, C.POINTER(Info)],
        "mdb_txn_begin": [handle, handle, C.c_uint, C.POINTER(handle)],
        "mdb_txn_commit": [handle],
        "mdb_put": [handle, C.c_uint, C.POINTER(Value), C.POINTER(Value), C.c_uint],
    }.items():
        function = getattr(lib, name)
        function.argtypes, function.restype = arguments, C.c_int
    lib.mdb_env_close.argtypes, lib.mdb_env_close.restype = [handle], None
    lib.mdb_txn_abort.argtypes, lib.mdb_txn_abort.restype = [handle], None
    return lib


def check(code):
    if code:
        raise RuntimeError(f"C API returned {code}")


def environment(lib, path, size):
    env = C.c_void_p()
    check(lib.mdb_env_create(C.byref(env)))
    try:
        check(lib.mdb_env_set_mapsize(env, size))
        check(lib.mdb_env_open(env, os.fsencode(path), 0x200000, 0o600))
        return env
    except BaseException:
        lib.mdb_env_close(env)
        raise


def capacity(lib, env):
    info = Info()
    check(lib.mdb_env_info(env, C.byref(info)))
    return info.mapsize


def commit_value(lib, env, data):
    txn = C.c_void_p()
    check(lib.mdb_txn_begin(env, None, 0, C.byref(txn)))
    key_bytes = C.create_string_buffer(b"capacity")
    value_bytes = C.create_string_buffer(data)
    key = Value(8, C.cast(key_bytes, C.c_void_p))
    value = Value(len(data), C.cast(value_bytes, C.c_void_p))
    try:
        check(lib.mdb_put(txn, 1, C.byref(key), C.byref(value), 0))
        result = lib.mdb_txn_commit(txn)
        txn = C.c_void_p()
        check(result)
    finally:
        if txn:
            lib.mdb_txn_abort(txn)


def child(path):
    lib = library()
    env = environment(lib, path, 16 * MIB)
    try:
        print(json.dumps({"opened": capacity(lib, env)}), flush=True)
        if sys.stdin.readline().strip() != "adopt":
            raise RuntimeError("Missing parent adoption command")
        before = capacity(lib, env)
        check(lib.mdb_env_set_mapsize(env, 0))
        print(json.dumps({"before": before, "after": capacity(lib, env)}), flush=True)
    finally:
        lib.mdb_env_close(env)


class MapCapacityTests(unittest.TestCase):
    def test_malformed_published_capacity_fails_closed(self):
        lib = library()
        with tempfile.TemporaryDirectory(prefix="ulmdb-map-corruption-") as temporary:
            for index, contents in enumerate((b"", b"\0" * 8, b"\1" * 7, b"\1" * 9)):
                path = Path(temporary) / str(index)
                env = environment(lib, path, 32 * MIB)
                try:
                    commit_value(lib, env, b"valid")
                finally:
                    lib.mdb_env_close(env)
                (path / "LMDB-MAPSIZE").write_bytes(contents)
                with self.assertRaisesRegex(RuntimeError, "-30796"):
                    environment(lib, path, 16 * MIB)

    def test_existing_process_adopts_newly_published_capacity(self):
        lib = library()
        with tempfile.TemporaryDirectory(prefix="ulmdb-map-capacity-") as temporary:
            path = Path(temporary) / "database"
            env = environment(lib, path, 32 * MIB)
            process = None
            try:
                commit_value(lib, env, b"initial")
                implicit = environment(lib, path, 0)
                self.assertEqual(capacity(lib, implicit), 32 * MIB)
                lib.mdb_env_close(implicit)
                process = subprocess.Popen([sys.executable, __file__, "--child", str(path)],
                                           stdin=subprocess.PIPE, stdout=subprocess.PIPE,
                                           stderr=subprocess.PIPE, text=True)
                ready, _, _ = select.select([process.stdout], [], [], 30)
                self.assertTrue(ready, "child environment open timed out")
                opened = json.loads(process.stdout.readline())
                self.assertEqual(opened["opened"], 16 * MIB)
                check(lib.mdb_env_set_mapsize(env, 64 * MIB))
                commit_value(lib, env, b"expanded")
                process.stdin.write("adopt\n")
                process.stdin.flush()
                stdout, stderr = process.communicate(timeout=30)
                self.assertEqual(process.returncode, 0, stderr)
                adopted = json.loads(stdout)
                self.assertEqual(adopted, {"before": 16 * MIB, "after": 64 * MIB})
                # A later commit from a smaller local reservation cannot erase
                # the globally advertised expansion.
                check(lib.mdb_env_set_mapsize(env, 48 * MIB))
                commit_value(lib, env, b"smaller-local")
                check(lib.mdb_env_set_mapsize(env, 0))
                self.assertEqual(capacity(lib, env), 64 * MIB)
            finally:
                lib.mdb_env_close(env)
                if process is not None and process.poll() is None:
                    process.kill()
                    process.communicate()


if __name__ == "__main__":
    if len(sys.argv) == 3 and sys.argv[1] == "--child":
        child(Path(sys.argv[2]))
    else:
        unittest.main()
