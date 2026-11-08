"""Concurrent initialization regression tests for the C environment adapter."""
import ctypes as C
import os
from pathlib import Path
import tempfile
import threading
import unittest
from concurrent.futures import ThreadPoolExecutor

ROOT = Path(__file__).resolve().parents[3]


def library():
    lib = C.CDLL(os.environ.get("ULMDB_LIBRARY", str(ROOT / "target/debug/libulmdb_c.so")))
    lib.mdb_env_create.argtypes = [C.POINTER(C.c_void_p)]
    lib.mdb_env_create.restype = C.c_int
    lib.mdb_env_open.argtypes = [C.c_void_p, C.c_char_p, C.c_uint, C.c_uint]
    lib.mdb_env_open.restype = C.c_int
    lib.mdb_env_close.argtypes = [C.c_void_p]
    lib.mdb_env_close.restype = None
    return lib


class InitializationTests(unittest.TestCase):
    def concurrent_opens(self, paths):
        lib = library()
        start = threading.Barrier(len(paths))
        opened = threading.Barrier(len(paths))

        def worker(path):
            handle = C.c_void_p()
            self.assertEqual(lib.mdb_env_create(C.byref(handle)), 0)
            try:
                start.wait(timeout=20)
                result = lib.mdb_env_open(handle, os.fsencode(path), 0, 0o600)
                # Keep successful environments open so another thread cannot
                # reopen the same directory before its peer has tried opening.
                opened.wait(timeout=20)
                return result
            finally:
                lib.mdb_env_close(handle)

        with ThreadPoolExecutor(max_workers=len(paths)) as pool:
            return list(pool.map(worker, paths))

    def test_different_empty_directories_with_same_stem_do_not_share_staging(self):
        with tempfile.TemporaryDirectory() as temp:
            paths = [Path(temp) / "database.a", Path(temp) / "database.b"]
            before = {}
            for path, mode in zip(paths, (0o750, 0o711)):
                path.mkdir()
                path.chmod(mode)
                stat = path.stat()
                before[path] = (stat.st_dev, stat.st_ino, stat.st_mode & 0o7777)
            self.assertEqual(self.concurrent_opens(paths), [0, 0])
            for path in paths:
                stat = path.stat()
                self.assertEqual((stat.st_dev, stat.st_ino, stat.st_mode & 0o7777), before[path])
            self.assertEqual(sorted(p.name for p in Path(temp).iterdir()), ["database.a", "database.b"])

    def test_same_empty_directory_initializes_consistently_and_reopens(self):
        with tempfile.TemporaryDirectory() as temp:
            path = Path(temp) / "database"
            path.mkdir()
            results = self.concurrent_opens([path, path])
            self.assertGreaterEqual(results.count(0), 1, results)
            lib = library()
            handle = C.c_void_p()
            self.assertEqual(lib.mdb_env_create(C.byref(handle)), 0)
            try:
                self.assertEqual(lib.mdb_env_open(handle, os.fsencode(path), 0, 0o600), 0)
            finally:
                lib.mdb_env_close(handle)

    def test_unrelated_directory_is_preserved(self):
        with tempfile.TemporaryDirectory() as temp:
            path = Path(temp) / "database"
            path.mkdir()
            marker = path / "application.txt"
            marker.write_bytes(b"keep me")
            lib = library()
            handle = C.c_void_p()
            self.assertEqual(lib.mdb_env_create(C.byref(handle)), 0)
            try:
                self.assertNotEqual(lib.mdb_env_open(handle, os.fsencode(path), 0, 0o600), 0)
                self.assertEqual(marker.read_bytes(), b"keep me")
                self.assertEqual([p.name for p in path.iterdir()], ["application.txt"])
            finally:
                lib.mdb_env_close(handle)

    def test_native_filesystem_paths_preserve_non_utf8_bytes(self):
        with tempfile.TemporaryDirectory() as temp:
            path=Path(temp)/os.fsdecode(b"database-\xff")
            lib=library()
            for _ in range(2):
                handle=C.c_void_p()
                self.assertEqual(lib.mdb_env_create(C.byref(handle)),0)
                try:
                    self.assertEqual(lib.mdb_env_open(handle,os.fsencode(path),0,0o600),0)
                    self.assertTrue((path/"CURRENT").is_file())
                finally:
                    lib.mdb_env_close(handle)


if __name__ == "__main__":
    unittest.main()
