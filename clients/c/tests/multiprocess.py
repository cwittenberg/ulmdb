"""Exercise independent processes using the public LMDB-compatible C ABI.

Run after building libulmdb_c. Spawned children open their own environments;
neither live native handles nor inherited writer capabilities cross a fork.
"""
from __future__ import annotations

import ctypes as C
import multiprocessing as MP
import os
from pathlib import Path
import sys
import tempfile
import unittest


ROOT = Path(__file__).resolve().parents[3]
READONLY = 0x20000
CREATE = 0x40000
NOTLS = 0x200000


class Value(C.Structure):
    _fields_ = [("size", C.c_size_t), ("data", C.c_void_p)]


def input_value(data):
    storage = C.create_string_buffer(data)
    return Value(len(data), C.cast(storage, C.c_void_p)), storage


class Native:
    def __init__(self, path, max_readers=None):
        suffix = ".dylib" if sys.platform == "darwin" else ".so"
        filename = os.environ.get("ULMDB_LIBRARY", str(ROOT / "target/debug" / f"libulmdb_c{suffix}"))
        self.lib = C.CDLL(filename)
        handle = C.c_void_p
        signatures = {
            "mdb_env_create": ([C.POINTER(handle)], C.c_int),
            "mdb_env_set_maxdbs": ([handle, C.c_uint], C.c_int),
            "mdb_env_set_maxreaders": ([handle, C.c_uint], C.c_int),
            "mdb_env_get_maxreaders": ([handle, C.POINTER(C.c_uint)], C.c_int),
            "mdb_env_open": ([handle, C.c_char_p, C.c_uint, C.c_uint], C.c_int),
            "mdb_env_close": ([handle], None),
            "mdb_txn_begin": ([handle, handle, C.c_uint, C.POINTER(handle)], C.c_int),
            "mdb_txn_commit": ([handle], C.c_int),
            "mdb_txn_abort": ([handle], None),
            "mdb_txn_reset": ([handle], None),
            "mdb_txn_renew": ([handle], C.c_int),
            "mdb_dbi_open": ([handle, C.c_char_p, C.c_uint, C.POINTER(C.c_uint)], C.c_int),
            "mdb_get": ([handle, C.c_uint, C.POINTER(Value), C.POINTER(Value)], C.c_int),
            "mdb_put": ([handle, C.c_uint, C.POINTER(Value), C.POINTER(Value), C.c_uint], C.c_int),
            "mdb_strerror": ([C.c_int], C.c_char_p),
            "mdb_reader_check": ([handle, C.POINTER(C.c_int)], C.c_int),
        }
        for name, (args, result) in signatures.items():
            function = getattr(self.lib, name)
            function.argtypes, function.restype = args, result
        self.env = handle()
        self.check(self.lib.mdb_env_create(C.byref(self.env)))
        try:
            self.check(self.lib.mdb_env_set_maxdbs(self.env, 8))
            if max_readers is not None:
                self.check(self.lib.mdb_env_set_maxreaders(self.env, max_readers))
            self.check(self.lib.mdb_env_open(self.env, os.fsencode(path), NOTLS, 0o600))
        except BaseException:
            self.close()
            raise

    def check(self, code):
        if code:
            raise RuntimeError(f"C ABI error {code}: {self.lib.mdb_strerror(code).decode()}")

    def begin(self, write=False):
        txn = C.c_void_p()
        self.check(self.lib.mdb_txn_begin(self.env, None, 0 if write else READONLY, C.byref(txn)))
        return txn

    def database(self, txn, create=False):
        db = C.c_uint()
        self.check(self.lib.mdb_dbi_open(txn, b"records", CREATE if create else 0, C.byref(db)))
        return db.value

    def get(self, txn, db, key=b"key"):
        native_key, key_storage = input_value(key)
        value = Value()
        self.check(self.lib.mdb_get(txn, db, C.byref(native_key), C.byref(value)))
        return C.string_at(value.data, value.size)

    def put(self, txn, db, data, key=b"key"):
        native_key, key_storage = input_value(key)
        native_value, value_storage = input_value(data)
        self.check(self.lib.mdb_put(txn, db, C.byref(native_key), C.byref(native_value), 0))

    def commit(self, txn):
        code = self.lib.mdb_txn_commit(txn)
        txn.value = None
        self.check(code)

    def close(self):
        if self.env:
            self.lib.mdb_env_close(self.env)
            self.env.value = None


def reader_worker(path, pipe):
    native, txn = None, C.c_void_p()
    try:
        native = Native(path)
        pipe.send("ready")
        while True:
            command = pipe.recv()
            if command == "begin":
                txn = native.begin()
                db = native.database(txn)
                pipe.send(native.get(txn, db))
            elif command == "read":
                pipe.send(native.get(txn, db))
            elif command == "renew":
                native.lib.mdb_txn_reset(txn)
                native.check(native.lib.mdb_txn_renew(txn))
                pipe.send(native.get(txn, db))
            elif command == "close":
                return
    except BaseException as error:
        pipe.send(("error", repr(error)))
    finally:
        if native:
            if txn:
                native.lib.mdb_txn_abort(txn)
            native.close()
        pipe.close()


def writer_worker(path, pipe, iterations):
    native, txn = None, C.c_void_p()
    try:
        native = Native(path)
        pipe.send("ready")
        pipe.recv()
        for _ in range(iterations):
            txn = native.begin(write=True)
            db = native.database(txn)
            count = int(native.get(txn, db, b"counter"))
            native.put(txn, db, str(count + 1).encode(), b"counter")
            native.commit(txn)
        pipe.send("done")
    except BaseException as error:
        pipe.send(("error", repr(error)))
    finally:
        if native:
            if txn:
                native.lib.mdb_txn_abort(txn)
            native.close()
        pipe.close()


def crashed_writer_worker(path, pipe):
    try:
        native = Native(path)
        txn = native.begin(write=True)
        db = native.database(txn)
        native.put(txn, db, b"never-committed")
        pipe.send("private-write-ready")
        pipe.close()
        # Exercise OS release of both writer coordination and engine fencing,
        # rather than a graceful transaction destructor.
        os._exit(0)
    except BaseException as error:
        pipe.send(("error", repr(error)))
        pipe.close()


class MultiprocessTests(unittest.TestCase):
    def setUp(self):
        self.directory = tempfile.TemporaryDirectory(prefix="ulmdb-c-multiprocess-")
        self.path = str(Path(self.directory.name) / "database")
        self.native = Native(self.path)
        txn = self.native.begin(write=True)
        self.db = self.native.database(txn, create=True)
        self.native.put(txn, self.db, b"old")
        self.native.put(txn, self.db, b"0", b"counter")
        self.native.commit(txn)
        self.children = []

    def tearDown(self):
        for process, pipe in self.children:
            if process.is_alive():
                process.terminate()
            process.join(timeout=5)
            pipe.close()
        self.native.close()
        self.directory.cleanup()

    def spawn(self, function, *args):
        context = MP.get_context("spawn")
        parent, child = context.Pipe()
        process = context.Process(target=function, args=(self.path, child, *args))
        process.start()
        child.close()
        self.children.append((process, parent))
        return process, parent

    def receive(self, pipe):
        self.assertTrue(pipe.poll(8), "child operation blocked while another process owned a writer")
        result = pipe.recv()
        self.assertFalse(isinstance(result, tuple) and result[0] == "error", result)
        return result

    def test_new_reader_does_not_wait_for_writer_and_pins_until_renew(self):
        process, pipe = self.spawn(reader_worker)
        self.assertEqual(self.receive(pipe), "ready")
        txn = self.native.begin(write=True)
        try:
            self.native.put(txn, self.db, b"new")
            pipe.send("begin")
            self.assertEqual(self.receive(pipe), b"old")
            self.native.commit(txn)
            pipe.send("read")
            self.assertEqual(self.receive(pipe), b"old")
            pipe.send("renew")
            self.assertEqual(self.receive(pipe), b"new")
            pipe.send("close")
            process.join(timeout=5)
            self.assertEqual(process.exitcode, 0)
        finally:
            if txn:
                self.native.lib.mdb_txn_abort(txn)

    def test_new_process_can_open_environment_while_writer_is_active(self):
        txn = self.native.begin(write=True)
        try:
            self.native.put(txn, self.db, b"private")
            process, pipe = self.spawn(reader_worker)
            self.assertEqual(self.receive(pipe), "ready")
            pipe.send("begin")
            self.assertEqual(self.receive(pipe), b"old")
            pipe.send("close")
            process.join(timeout=5)
            self.assertEqual(process.exitcode, 0)
        finally:
            self.native.lib.mdb_txn_abort(txn)

    def test_independent_writers_serialize_without_lost_updates(self):
        workers = [self.spawn(writer_worker, 12) for _ in range(2)]
        for _, pipe in workers:
            self.assertEqual(self.receive(pipe), "ready")
        for _, pipe in workers:
            pipe.send("start")
        for process, pipe in workers:
            self.assertEqual(self.receive(pipe), "done")
            process.join(timeout=5)
            self.assertEqual(process.exitcode, 0)
        txn = self.native.begin()
        try:
            self.assertEqual(self.native.get(txn, self.db, b"counter"), b"24")
        finally:
            self.native.lib.mdb_txn_abort(txn)

    def test_crashed_writer_releases_fences_and_discards_private_data(self):
        process, pipe = self.spawn(crashed_writer_worker)
        self.assertEqual(self.receive(pipe), "private-write-ready")
        process.join(timeout=5)
        self.assertEqual(process.exitcode, 0)
        txn = self.native.begin(write=True)
        try:
            self.assertEqual(self.native.get(txn, self.db), b"old")
            self.native.put(txn, self.db, b"after-crash")
            self.native.commit(txn)
        finally:
            if txn:
                self.native.lib.mdb_txn_abort(txn)


if __name__ == "__main__":
    unittest.main()
