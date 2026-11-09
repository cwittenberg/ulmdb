#!/usr/bin/env python3
"""Commit-capacity refusal for the owned module's selected versioned profile.

The default 64 MiB index profile has a 32 MiB versioned value pool. Three
12 MiB values fit the adapter's 64 MiB SQL staging limit, but cannot all be
admitted by the native transaction. This is a real commit refusal, not an
injected success/failure or a reduced durability setting.
"""
import pathlib
import sqlite3
import sys
import tempfile

from test import connect


def main():
    extension = pathlib.Path(sys.argv[1]).resolve()
    with tempfile.TemporaryDirectory(prefix="ulmdb-owned-full-") as directory:
        root = pathlib.Path(directory)
        catalog = root / "catalog.sqlite"
        db = connect(catalog, extension)
        db.execute(f"CREATE VIRTUAL TABLE kv USING ulmdb_owned('{root}/store')")
        db.execute("INSERT INTO kv VALUES(1,x'73656E74696E656C')")
        db.commit()
        db.execute("BEGIN")
        for key in (100, 101, 102):
            db.execute("INSERT INTO kv VALUES(?,zeroblob(12582912))", (key,))
        try:
            db.commit()
        except sqlite3.Error as error:
            assert error.sqlite_errorcode & 255 == sqlite3.SQLITE_FULL, error
        else:
            raise AssertionError("expected native versioned-pool capacity refusal at COMMIT")
        if db.in_transaction:
            db.rollback()
        assert db.execute("SELECT id,value FROM kv ORDER BY id").fetchall() == [(1, b"sentinel")]
        db.execute("INSERT INTO kv VALUES(2,x'6F6B')")
        db.commit()
        db.close()
        db = connect(catalog, extension)
        assert db.execute("SELECT id,value FROM kv ORDER BY id").fetchall() == [(1, b"sentinel"), (2, b"ok")]
        db.close()
    print("SQLite owned versioned profile: native COMMIT capacity refusal reaches SQL, old data survives, next commit and reopen pass")


if __name__ == "__main__":
    main()
