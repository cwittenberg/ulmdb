#!/usr/bin/env python3
"""Owned-engine SQL isolation, writer reservation and process-fence checks."""
import pathlib
import sqlite3
import subprocess
import sys
import tempfile
from concurrent.futures import ThreadPoolExecutor

from test import connect


def attach(db, directory):
    quoted = str(directory).replace("'", "''")
    db.execute(f"CREATE VIRTUAL TABLE kv USING ulmdb_owned('{quoted}')")


def child_open(extension, directory, expect_open):
    script = """
import pathlib,sqlite3,sys
db=sqlite3.connect(':memory:')
db.enable_load_extension(True)
db.load_extension(sys.argv[1])
path=sys.argv[2].replace("'", "''")
expected=sys.argv[3]=='yes'
try:
    db.execute(f"CREATE VIRTUAL TABLE kv USING ulmdb_owned('{path}')")
except sqlite3.Error as error:
    if expected: raise
    print('owned process fence refused competing owner:',error)
else:
    if not expected: raise AssertionError('competing process opened active owned store')
    print('owned process fence released:',db.execute('SELECT count(*) FROM kv').fetchone()[0])
db.close()
"""
    result = subprocess.run([sys.executable, "-c", script, str(extension), str(directory),
                             "yes" if expect_open else "no"],
                            capture_output=True, text=True, timeout=20)
    assert result.returncode == 0, result.stdout + result.stderr
    assert "process fence" in result.stdout, result.stdout


def main():
    extension = pathlib.Path(sys.argv[1]).resolve()
    with tempfile.TemporaryDirectory(prefix="ulmdb-owned-sql-") as temporary:
        root = pathlib.Path(temporary)
        store = root / "store"
        # Separate SQLite catalogs ensure ULMDB, rather than a catalog lock,
        # arbitrates concurrent writers sharing this owned environment.
        writer = connect(root / "writer.sqlite", extension)
        reader = connect(root / "reader.sqlite", extension)
        attach(writer, store)
        attach(reader, store)
        writer.executemany("INSERT INTO kv VALUES(?,?)", [(1, b"old"), (2, b"two"), (3, b"")])
        writer.commit()
        writer.execute("BEGIN")
        writer.execute("UPDATE kv SET value=x'6E6577' WHERE id=1")
        assert writer.execute("SELECT value FROM kv WHERE id=1").fetchone() == (b"new",)
        assert reader.execute("SELECT value FROM kv WHERE id=1").fetchone() == (b"old",)
        try:
            reader.execute("INSERT INTO kv VALUES(4,x'04')")
        except sqlite3.OperationalError as error:
            assert error.sqlite_errorcode & 255 == sqlite3.SQLITE_BUSY, error
        else:
            raise AssertionError("second writer accepted while first writer was active")
        reader.rollback()
        writer.commit()
        assert reader.execute("SELECT value FROM kv WHERE id=1").fetchone() == (b"new",)
        reader.execute("INSERT INTO kv VALUES(4,x'04')")
        reader.commit()
        assert writer.execute("SELECT id FROM kv WHERE id=4").fetchone() == (4,)

        batch = reader.execute("SELECT id,value FROM kv WHERE id IN(1,2,3,4) ORDER BY id")
        assert batch.fetchone() == (1, b"new")
        writer.execute("UPDATE kv SET value=x'6368616E676564' WHERE id=2")
        writer.commit()
        assert batch.fetchall() == [(2, b"two"), (3, b""), (4, b"\x04")]
        assert reader.execute("SELECT value FROM kv WHERE id=2").fetchone() == (b"changed",)

        threaded = connect(root / "threaded.sqlite", extension, check_same_thread=False)
        attach(threaded, store)
        first = threaded.execute("SELECT id,value FROM kv ORDER BY id")
        second = threaded.execute("SELECT id,value FROM kv WHERE id IN(1,2,3,4) ORDER BY id")
        assert first.fetchone() == second.fetchone() == (1, b"new")
        first.close()
        second.close()

        def point():
            cursor = threaded.execute("SELECT value FROM kv WHERE id=2")
            try:
                return cursor.fetchone()
            finally:
                cursor.close()

        # Completed queries may reuse empty allocation headers across threads;
        # each renewed reader must capture the latest actual committed snapshot.
        for generation in range(8):
            value = f"generation-{generation}".encode()
            writer.execute("UPDATE kv SET value=? WHERE id=2", (value,))
            writer.commit()
            with ThreadPoolExecutor(max_workers=1) as executor:
                assert executor.submit(point).result(timeout=10) == (value,)
        writer.execute("UPDATE kv SET value=? WHERE id=2", (b"changed",))
        writer.commit()
        assert point() == (b"changed",)
        child_open(extension, store, False)
        reader.close()
        writer.close()
        child_open(extension, store, False)
        threaded.close()
        child_open(extension, store, True)
        reopened = connect(root / "writer.sqlite", extension)
        assert reopened.execute("SELECT id,value FROM kv ORDER BY id").fetchall() == [
            (1, b"new"), (2, b"changed"), (3, b""), (4, b"\x04")]
        reopened.close()
    print("SQLite owned adapter: shared-process snapshots, writer BUSY, fresh reads across threads, empty-carrier teardown, exclusive process fence, release and durable reopen passed")


if __name__ == "__main__":
    main()
