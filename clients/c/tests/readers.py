#!/usr/bin/env python3
"""Shared reader capacity, reset-slot reservation and dead-reader cleanup."""
import ctypes as C
import multiprocessing as MP
import os
from pathlib import Path
import tempfile
import threading
import unittest

from multiprocess import Native


class EnvironmentInfo(C.Structure):
    _fields_ = [
        ("mapaddr", C.c_void_p),
        ("mapsize", C.c_size_t),
        ("last_pgno", C.c_size_t),
        ("last_txnid", C.c_size_t),
        ("maxreaders", C.c_uint),
        ("numreaders", C.c_uint),
    ]


def reader_high_water(native):
    native.lib.mdb_env_info.argtypes = [C.c_void_p, C.POINTER(EnvironmentInfo)]
    native.lib.mdb_env_info.restype = C.c_int
    info = EnvironmentInfo()
    native.check(native.lib.mdb_env_info(native.env, C.byref(info)))
    return info.numreaders


def capacity(native):
    result = C.c_uint()
    native.check(native.lib.mdb_env_get_maxreaders(native.env, C.byref(result)))
    return result.value


def lease_worker(path, pipe, requested):
    native, txn = None, C.c_void_p()
    try:
        native = Native(path, max_readers=requested)
        pipe.send(capacity(native))
        while True:
            command = pipe.recv()
            if command == "begin":
                txn = native.begin()
                pipe.send("begun")
            elif command == "high-water":
                pipe.send(reader_high_water(native))
            elif command == "close":
                if txn:
                    native.lib.mdb_txn_abort(txn)
                    txn.value = None
                native.close()
                pipe.send("closed")
                return
            elif command == "crash":
                pipe.send("exiting")
                pipe.close()
                os._exit(0)
    except BaseException as error:
        pipe.send(("error", repr(error)))
    finally:
        if native:
            if txn:
                native.lib.mdb_txn_abort(txn)
            native.close()
        pipe.close()


class ReaderLeaseTests(unittest.TestCase):
    def setUp(self):
        self.directory = tempfile.TemporaryDirectory(prefix="ulmdb-reader-slots-")
        self.path = str(Path(self.directory.name) / "database")
        self.native = Native(self.path, max_readers=2)
        self.children = []

    def tearDown(self):
        for process, pipe in self.children:
            if process.is_alive():
                process.terminate()
            process.join(timeout=5)
            pipe.close()
        self.native.close()
        self.directory.cleanup()

    def spawn(self, requested):
        context = MP.get_context("spawn")
        parent, child = context.Pipe()
        process = context.Process(target=lease_worker, args=(self.path, child, requested))
        process.start()
        child.close()
        self.children.append((process, parent))
        return process, parent

    def receive(self, pipe):
        self.assertTrue(pipe.poll(8), "reader capability operation blocked")
        result = pipe.recv()
        self.assertFalse(isinstance(result, tuple) and result[0] == "error", result)
        return result

    def close_child(self, process, pipe):
        pipe.send("close")
        self.assertEqual(self.receive(pipe), "closed")
        process.join(timeout=5)
        self.assertEqual(process.exitcode, 0)

    def test_first_opener_limit_is_shared_and_later_exclusive_opener_can_only_grow(self):
        self.assertEqual(capacity(self.native), 2)
        process, pipe = self.spawn(4)
        self.assertEqual(self.receive(pipe), 2)
        self.close_child(process, pipe)
        # An active transaction retains its environment capability even if the
        # caller closes its separate environment reference only after abort.
        self.native.close()
        self.native = Native(self.path, max_readers=4)
        self.assertEqual(capacity(self.native), 4)
        self.native.close()
        self.native = Native(self.path, max_readers=1)
        self.assertEqual(capacity(self.native), 4)

    def test_reset_keeps_global_slot_and_renew_reuses_it(self):
        self.assertEqual(reader_high_water(self.native), 0)
        process, pipe = self.spawn(4)
        self.assertEqual(self.receive(pipe), 2)
        pipe.send("begin")
        self.assertEqual(self.receive(pipe), "begun")
        self.assertEqual(reader_high_water(self.native), 1)
        txn = self.native.begin()
        other = C.c_void_p()
        try:
            self.native.lib.mdb_txn_reset(txn)
            self.assertEqual(reader_high_water(self.native), 2)
            pipe.send("high-water")
            self.assertEqual(self.receive(pipe), 2)
            # Only one reader is active, but both table slots are reserved.
            result = self.native.lib.mdb_txn_begin(self.native.env, None, 0x20000, C.byref(other))
            self.assertEqual(result, -30790)
            self.assertFalse(other)
            self.native.check(self.native.lib.mdb_txn_renew(txn))
            self.native.lib.mdb_txn_reset(txn)
            self.native.check(self.native.lib.mdb_txn_renew(txn))
        finally:
            self.native.lib.mdb_txn_abort(txn)
            if other:
                self.native.lib.mdb_txn_abort(other)
        self.close_child(process, pipe)
        self.assertEqual(reader_high_water(self.native), 2)
        # Aborting the reset/reused transaction finally releases its slot.
        txn = self.native.begin()
        self.assertEqual(reader_high_water(self.native), 2)
        self.native.lib.mdb_txn_abort(txn)

    def test_dead_reader_is_counted_and_its_slot_is_reusable(self):
        process, pipe = self.spawn(2)
        self.assertEqual(self.receive(pipe), 2)
        pipe.send("begin")
        self.assertEqual(self.receive(pipe), "begun")
        self.assertEqual(reader_high_water(self.native), 1)
        pipe.send("crash")
        self.assertEqual(self.receive(pipe), "exiting")
        process.join(timeout=5)
        self.assertEqual(process.exitcode, 0)
        dead = C.c_int()
        self.native.check(self.native.lib.mdb_reader_check(self.native.env, C.byref(dead)))
        self.assertEqual(dead.value, 1)
        self.assertEqual(reader_high_water(self.native), 1)
        self.native.check(self.native.lib.mdb_reader_check(self.native.env, C.byref(dead)))
        self.assertEqual(dead.value, 0)
        first, second = self.native.begin(), self.native.begin()
        try:
            third = C.c_void_p()
            self.assertEqual(self.native.lib.mdb_txn_begin(self.native.env, None, 0x20000, C.byref(third)), -30790)
        finally:
            self.native.lib.mdb_txn_abort(first)
            self.native.lib.mdb_txn_abort(second)
        self.assertEqual(reader_high_water(self.native), 2)

    def test_high_water_survives_shared_reopen_but_resets_after_exclusive_reopen(self):
        first, second = self.native.begin(), self.native.begin()
        self.native.lib.mdb_txn_abort(first)
        self.native.lib.mdb_txn_abort(second)
        process, pipe = self.spawn(2)
        self.assertEqual(self.receive(pipe), 2)
        pipe.send("high-water")
        self.assertEqual(self.receive(pipe), 2)
        self.native.close()
        self.native = Native(self.path, max_readers=2)
        self.assertEqual(reader_high_water(self.native), 2)
        self.close_child(process, pipe)
        self.native.close()
        self.native = Native(self.path, max_readers=2)
        self.assertEqual(reader_high_water(self.native), 0)
        txn = self.native.begin()
        self.assertEqual(reader_high_water(self.native), 1)
        self.native.lib.mdb_txn_abort(txn)

    def test_dead_slots_from_previous_session_are_not_reported_after_reopen(self):
        process, pipe = self.spawn(2)
        self.assertEqual(self.receive(pipe), 2)
        pipe.send("begin")
        self.assertEqual(self.receive(pipe), "begun")
        pipe.send("crash")
        self.assertEqual(self.receive(pipe), "exiting")
        process.join(timeout=5)
        self.assertEqual(process.exitcode, 0)
        self.native.close()
        self.native = Native(self.path, max_readers=2)
        self.assertEqual(reader_high_water(self.native), 0)
        dead = C.c_int()
        self.native.check(self.native.lib.mdb_reader_check(self.native.env, C.byref(dead)))
        self.assertEqual(dead.value, 0)

    def test_tls_reset_abort_and_thread_exit_do_not_lower_high_water(self):
        self.native.close()
        self.native.check(self.native.lib.mdb_env_create(C.byref(self.native.env)))
        self.native.check(self.native.lib.mdb_env_set_maxreaders(self.native.env, 2))
        self.native.check(self.native.lib.mdb_env_open(self.native.env, os.fsencode(self.path), 0, 0o600))
        txn = self.native.begin()
        try:
            self.native.lib.mdb_txn_reset(txn)
            self.assertEqual(reader_high_water(self.native), 1)
            self.native.check(self.native.lib.mdb_txn_renew(txn))
        finally:
            self.native.lib.mdb_txn_abort(txn)
        self.assertEqual(reader_high_water(self.native), 1)
        errors = []

        def use_thread_slot():
            try:
                thread_txn = self.native.begin()
                self.native.lib.mdb_txn_abort(thread_txn)
            except BaseException as error:
                errors.append(error)

        for _ in range(2):
            thread = threading.Thread(target=use_thread_slot)
            thread.start()
            thread.join(timeout=5)
            self.assertFalse(thread.is_alive())
            self.assertEqual(errors, [])
            self.assertEqual(reader_high_water(self.native), 2)

    def test_malformed_shared_high_water_is_rejected_and_exclusive_open_reinitializes(self):
        registry = Path(self.path) / "lmdb-readers" / "registry.lock"
        original = registry.read_bytes()
        self.native.lib.mdb_env_info.argtypes = [C.c_void_p, C.POINTER(EnvironmentInfo)]
        self.native.lib.mdb_env_info.restype = C.c_int
        info = EnvironmentInfo()
        for invalid in [b"", b"\x01", (3).to_bytes(4, "little"), b"\0" * 5]:
            try:
                registry.write_bytes(invalid)
                self.assertEqual(self.native.lib.mdb_env_info(self.native.env, C.byref(info)), -30796)
                txn = C.c_void_p()
                self.assertEqual(self.native.lib.mdb_txn_begin(self.native.env, None, 0x20000, C.byref(txn)), -30796)
                self.assertFalse(txn)
            finally:
                registry.write_bytes(original)
        registry.write_bytes(b"corrupt")
        self.native.close()
        self.native = Native(self.path, max_readers=2)
        self.assertEqual(reader_high_water(self.native), 0)


if __name__ == "__main__":
    unittest.main()
