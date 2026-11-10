#!/usr/bin/env python3
"""Normal SQLite SQL through the optional page VFS; no core changes."""
import argparse
import concurrent.futures
import ctypes
import os
import pathlib
import sqlite3
import stat
import subprocess
import sys
import tempfile
import urllib.parse


def register(extension):
    bootstrap = sqlite3.connect(":memory:")
    bootstrap.enable_load_extension(True)
    bootstrap.load_extension(str(extension))
    bootstrap.enable_load_extension(False)
    return bootstrap


def connect(path, *, readonly=False, cross_thread=False):
    uri = "file:" + urllib.parse.quote(str(path), safe="/") + "?vfs=ulmdb"
    if readonly:
        uri += "&mode=ro"
    db = sqlite3.connect(uri, uri=True, isolation_level=None,
                         check_same_thread=not cross_thread)
    db.execute("PRAGMA busy_timeout=0")
    return db


def stored_size(extension, path):
    """Read persisted VFS metadata, independently of SQLite's header count."""
    library = ctypes.CDLL(str(extension))
    library.upg_open.argtypes = [ctypes.c_char_p, ctypes.c_int,
                                 ctypes.POINTER(ctypes.c_void_p)]
    library.upg_size.argtypes = [ctypes.c_void_p, ctypes.POINTER(ctypes.c_int64)]
    library.upg_close.argtypes = [ctypes.c_void_p]
    handle = ctypes.c_void_p()
    assert library.upg_open(os.fsencode(path), 1, ctypes.byref(handle)) == 0  # SQLITE_OPEN_READONLY
    try:
        size = ctypes.c_int64()
        assert library.upg_size(handle, ctypes.byref(size)) == 0
        return size.value
    finally:
        assert library.upg_close(handle) == 0


def check_journal_privacy(root):
    previous_umask = os.umask(0o022)
    db = None
    try:
        store = root / "private.db"
        db = connect(store)
        db.execute("CREATE TABLE data(id INTEGER PRIMARY KEY,value BLOB)")
        db.execute("BEGIN")
        db.executemany("INSERT INTO data VALUES(?,?)", [(i, bytes([i % 251]) * 2048) for i in range(300)])
        db.execute("COMMIT")
        db.execute("PRAGMA cache_size=-8")
        db.execute("BEGIN IMMEDIATE")
        db.execute("UPDATE data SET value=zeroblob(2048)")
        journal = pathlib.Path(str(store) + "-journal")
        assert journal.is_file(), "dirty spill did not open a rollback journal"
        assert stat.S_IMODE(journal.stat().st_mode) & 0o077 == 0, oct(journal.stat().st_mode)
        assert stat.S_IMODE(store.stat().st_mode) == 0o700
        db.execute("ROLLBACK")
        assert db.execute("SELECT value FROM data WHERE id=1").fetchone() == (b"\x01" * 2048,)
    finally:
        if db:
            db.close()
        os.umask(previous_umask)


def check_page_sizes(root, extension):
    # SQLite pages can span, share, or exactly match a native 4 KiB block.
    for size in (512, 1024, 4096, 8192, 65536):
        store = root / f"pages-{size}.db"
        db = connect(store)
        db.execute(f"PRAGMA page_size={size}")
        db.execute("CREATE TABLE data(id INTEGER PRIMARY KEY,value BLOB)")
        db.execute("BEGIN")
        db.executemany("INSERT INTO data VALUES(?,?)", [(i, bytes([i]) * 1500) for i in range(8)])
        db.execute("COMMIT")
        db.execute("DELETE FROM data WHERE id<7")
        db.execute("VACUUM")
        expected = db.execute("PRAGMA page_count").fetchone()[0] * size
        db.close()
        assert stored_size(extension, store) == expected
        db = connect(store)
        assert db.execute("PRAGMA page_size").fetchone() == (size,)
        assert db.execute("SELECT * FROM data").fetchall() == [(7, b"\x07" * 1500)]
        assert db.execute("PRAGMA integrity_check").fetchall() == [("ok",)]
        db.close()


def child(extension, path, action):
    script = r'''
import os, pathlib, sys
sys.path.insert(0, sys.argv[1])
from test_vfs import register, connect
bootstrap = register(pathlib.Path(sys.argv[2]))
try:
    db = connect(pathlib.Path(sys.argv[3]))
except Exception:
    if sys.argv[4] == "refuse":
        sys.exit(0)
    raise
if sys.argv[4] == "refuse":
    raise AssertionError("another process acquired an owned page store")
if sys.argv[4] == "create-crash":
    db.execute("CREATE TABLE kv(id INTEGER PRIMARY KEY,value BLOB NOT NULL,tag TEXT)")
    db.execute("CREATE INDEX by_tag ON kv(tag)")
    db.execute("BEGIN")
    db.executemany("INSERT INTO kv VALUES(?,?,?)", [(i,bytes([i%251])*2048,str(i)) for i in range(300)])
    db.execute("COMMIT")
    db.execute("PRAGMA cache_size=-8")
    db.execute("BEGIN IMMEDIATE")
    db.execute("UPDATE kv SET value=zeroblob(2048),tag='pending'")
    os._exit(0)
if sys.argv[4] == "commit-crash":
    db.execute("BEGIN IMMEDIATE")
    db.execute("UPDATE kv SET value=x'ABCD',tag='committed'")
    db.execute("COMMIT")
    os._exit(0)
raise AssertionError(sys.argv[4])
'''
    result = subprocess.run([sys.executable, "-c", script,
                             str(pathlib.Path(__file__).resolve().parent),
                             str(extension), str(path), action],
                            capture_output=True, text=True)
    assert result.returncode == 0, result.stderr + result.stdout


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("extension", type=pathlib.Path)
    parser.add_argument("--expect-unsupported", action="store_true")
    args = parser.parse_args()
    extension = args.extension.resolve()
    if args.expect_unsupported:
        try:
            register(extension)
        except sqlite3.OperationalError as error:
            assert "ENABLE_BATCH_ATOMIC_WRITE" in str(error), error
        else:
            raise AssertionError("VFS loaded without required SQLite capability")
        print("SQLite page VFS: unsupported host fails closed")
        return
    bootstrap = register(extension)
    with tempfile.TemporaryDirectory(prefix="ulmdb-vfs-test-") as directory:
        root = pathlib.Path(directory)
        store = root / "normal.db"
        db = connect(store, cross_thread=True)
        assert db.execute("PRAGMA journal_mode").fetchone() == ("delete",)
        assert db.execute("PRAGMA journal_mode=DELETE").fetchone() == ("delete",)
        db.execute("PRAGMA synchronous=FULL")
        assert db.execute("PRAGMA synchronous").fetchone() == (2,)
        for setting in ("journal_mode=WAL", "journal_mode=OFF", "journal_mode=MEMORY",
                        "journal_mode=PERSIST", "journal_mode=TRUNCATE",
                        "synchronous=OFF", "synchronous=NORMAL", "synchronous=0", "synchronous=1"):
            try:
                db.execute("PRAGMA " + setting)
            except sqlite3.OperationalError as error:
                assert "ULMDB VFS requires" in str(error), (setting, error)
            else:
                raise AssertionError("unsafe mode accepted: " + setting)
        assert db.execute("PRAGMA mmap_size=268435456").fetchone() == (0,)
        db.execute("CREATE TABLE kv(id INTEGER PRIMARY KEY,value BLOB NOT NULL,tag TEXT UNIQUE)")
        db.execute("CREATE INDEX by_size ON kv(length(value))")
        rows = [(i, bytes([i]) * (i * 37), f"tag-{i}") for i in range(40)]
        db.execute("BEGIN")
        db.executemany("INSERT INTO kv VALUES(?,?,?)", rows)
        db.execute("SAVEPOINT nested")
        db.execute("UPDATE kv SET value=x'FF',tag='pending' WHERE id=7")
        db.execute("DELETE FROM kv WHERE id=8")
        db.execute("ROLLBACK TO nested")
        db.execute("RELEASE nested")
        db.execute("COMMIT")
        assert db.execute("SELECT * FROM kv ORDER BY id").fetchall() == rows
        assert db.execute("SELECT id FROM kv WHERE tag='tag-7'").fetchone() == (7,)
        plan = db.execute("EXPLAIN QUERY PLAN SELECT id FROM kv WHERE tag='tag-7'").fetchall()
        assert any("INDEX" in row[3] for row in plan), plan
        db.execute("BEGIN")
        db.execute("INSERT INTO kv VALUES(100,x'AA','new')")
        try:
            db.execute("INSERT INTO kv VALUES(101,x'BB','unique'),(102,x'CC','tag-7')")
        except sqlite3.IntegrityError:
            pass
        else:
            raise AssertionError("unique index admitted duplicates")
        assert db.execute("SELECT id FROM kv WHERE id=101").fetchall() == []
        db.execute("ROLLBACK")
        assert db.execute("SELECT * FROM kv ORDER BY id").fetchall() == rows
        assert db.execute("PRAGMA integrity_check").fetchall() == [("ok",)]
        db.execute("PRAGMA temp_store=FILE")
        db.execute("CREATE TEMP TABLE scratch AS SELECT * FROM kv")
        assert db.execute("SELECT count(*) FROM scratch").fetchone() == (40,)
        db.execute("DROP TABLE scratch")
        db.execute("VACUUM")
        assert db.execute("SELECT * FROM kv ORDER BY id").fetchall() == rows
        db.execute("CREATE TABLE reclaim(value BLOB)")
        db.execute("BEGIN")
        db.executemany("INSERT INTO reclaim VALUES(?)", [(b"page" * 2048,)] * 200)
        db.execute("COMMIT")
        expanded = stored_size(extension, store)
        db.execute("DROP TABLE reclaim")
        db.execute("VACUUM")
        expected_size = db.execute("PRAGMA page_count").fetchone()[0] * db.execute("PRAGMA page_size").fetchone()[0]
        db.close()
        actual_size = stored_size(extension, store)
        assert actual_size == expected_size < expanded, (actual_size, expected_size, expanded)
        db = connect(store, cross_thread=True)
        assert db.execute("SELECT * FROM kv ORDER BY id").fetchall() == rows
        scan = db.execute("SELECT id,value FROM kv ORDER BY id")
        assert scan.fetchone() == rows[0][:2]
        with concurrent.futures.ThreadPoolExecutor(max_workers=1) as pool:
            assert pool.submit(scan.fetchall).result() == [row[:2] for row in rows[1:]]
        other = connect(store)
        new_reader = connect(store)
        other.execute("BEGIN")
        assert other.execute("SELECT value FROM kv WHERE id=7").fetchone() == (rows[7][1],)
        db.execute("BEGIN IMMEDIATE")
        db.execute("UPDATE kv SET value=x'77' WHERE id=7")
        try:
            db.execute("COMMIT")
        except sqlite3.OperationalError as error:
            assert error.sqlite_errorcode == sqlite3.SQLITE_BUSY, error
        else:
            raise AssertionError("writer committed past a shared reader lock")
        try:
            new_reader.execute("SELECT value FROM kv WHERE id=7").fetchone()
        except sqlite3.OperationalError as error:
            assert error.sqlite_errorcode == sqlite3.SQLITE_BUSY, error
        else:
            raise AssertionError("new reader bypassed a waiting writer's PENDING lock")
        new_reader.close()
        assert other.execute("SELECT value FROM kv WHERE id=7").fetchone() == (rows[7][1],)
        other.execute("COMMIT")
        db.execute("COMMIT")
        assert other.execute("SELECT value FROM kv WHERE id=7").fetchone() == (b"\x77",)
        child(extension, store, "refuse")
        other.close()
        db.close()
        db = connect(store, readonly=True)
        assert db.execute("SELECT value FROM kv WHERE id=7").fetchone() == (b"\x77",)
        try:
            db.execute("INSERT INTO kv VALUES(100,x'AA','ro')")
        except sqlite3.OperationalError as error:
            assert error.sqlite_errorcode == sqlite3.SQLITE_READONLY, error
        else:
            raise AssertionError("readonly page store accepted a write")
        db.close()
        # Rejected normal SQLite files retain their bytes.
        ordinary = root / "ordinary.sqlite"
        reference = sqlite3.connect(ordinary)
        reference.execute("CREATE TABLE keep(value TEXT)")
        reference.commit()
        reference.close()
        before = ordinary.read_bytes()
        try:
            connect(ordinary)
        except sqlite3.OperationalError:
            pass
        else:
            raise AssertionError("page VFS opened an ordinary SQLite file")
        assert ordinary.read_bytes() == before
        check_page_sizes(root, extension)
        check_journal_privacy(root)
        crashed = root / "crash.db"
        child(extension, crashed, "create-crash")
        recovered = connect(crashed)
        expected = [(i, bytes([i % 251]) * 2048, str(i)) for i in range(300)]
        assert recovered.execute("SELECT * FROM kv ORDER BY id").fetchall() == expected
        assert recovered.execute("PRAGMA integrity_check").fetchall() == [("ok",)]
        recovered.close()
        child(extension, crashed, "commit-crash")
        recovered = connect(crashed)
        assert recovered.execute("SELECT count(*) FROM kv WHERE value=x'ABCD' AND tag='committed'").fetchone() == (300,)
        assert recovered.execute("PRAGMA integrity_check").fetchall() == [("ok",)]
        recovered.close()
    bootstrap.close()
    print("SQLite page VFS: normal schemas/indexes, SQL transactions/savepoints, locks, readonly/reopen, process fence, thread handoff, spill rollback and acknowledged-commit crash recovery passed")


if __name__ == "__main__":
    main()
