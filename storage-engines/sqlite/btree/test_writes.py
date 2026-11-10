#!/usr/bin/env python3
"""Whole-record native differential for lazy logical write positions."""
import hashlib
import json
import os
from pathlib import Path
import subprocess
import sys
import tempfile


def child(path):
    import sqlite3
    db = sqlite3.connect(path, isolation_level=None)
    db.execute('CREATE TABLE kv(id INTEGER PRIMARY KEY,value BLOB NOT NULL)')
    keys = [-(1 << 63), (1 << 63) - 1] + list(range(-30, 31))
    blob = lambda key: (hashlib.sha256(str(key).encode()).digest() * 7)[:64 + key % 137]
    results = []

    def capture():
        rows = db.execute('SELECT id,hex(value) FROM kv ORDER BY id').fetchall()
        results.append(rows)
        assert db.execute('SELECT id,hex(value) FROM kv ORDER BY id DESC').fetchall() == rows[::-1]

    db.execute('BEGIN')
    db.executemany('INSERT INTO kv VALUES(?,?)', [(key, blob(key)) for key in keys])
    capture()
    db.execute("UPDATE kv SET value=CAST('scan-'||id AS BLOB)")
    capture()
    db.executemany('UPDATE kv SET value=? WHERE id=?', [(blob(key)[::-1], key) for key in keys[::-1]])
    capture()
    db.execute('COMMIT')
    # Position restoration after the new logical key differs from the native
    # cursor's pre-write position, including subsequent rowid/scan operations.
    db.execute('UPDATE kv SET id=id+1000 WHERE id BETWEEN -10 AND 10')
    capture()
    db.execute('UPDATE kv SET id=id-2000 WHERE id BETWEEN 990 AND 1010')
    capture()
    db.execute("INSERT OR REPLACE INTO kv VALUES(-30,x'00010203'),(0,x'040506'),(30,x'ff')")
    capture()
    # The reserve/zero-fill and transfer optimizations keep their original path.
    db.execute('UPDATE kv SET value=zeroblob(97) WHERE id IN (-30,0,30)')
    capture()
    db.execute('CREATE TABLE copied(id INTEGER PRIMARY KEY,value BLOB NOT NULL)')
    db.execute('INSERT INTO copied SELECT * FROM kv')
    assert db.execute('SELECT id,hex(value) FROM copied ORDER BY id').fetchall() == results[-1]
    # A pending SQLite result owns stable bytes across another statement's
    # mutation, then its cursor resumes at the original logical rowid.
    db.execute('BEGIN')
    pending = db.execute('SELECT id,hex(value) FROM kv WHERE id>=-30 ORDER BY id')
    db.execute("UPDATE kv SET value=CAST('pending-'||id AS BLOB) WHERE id>=-30")
    results.append(pending.fetchall())
    db.execute('ROLLBACK')
    capture()
    # Incremental reads start from lazy exact read cursors; writes and blob
    # invalidation still use checked current positions.
    if hasattr(db, 'blobopen'):
        with db.blobopen('kv', 'value', -30, readonly=True) as handle:
            assert handle.read() == bytes(97)
        with db.blobopen('kv', 'value', -30, readonly=False) as handle:
            handle.seek(31)
            handle.write(bytes(range(32)))
            handle.seek(0)
            assert handle.read() == bytes(31) + bytes(range(32)) + bytes(34)
        capture()
    db.execute('DELETE FROM kv WHERE id BETWEEN -5 AND 25')
    capture()
    db.execute('SAVEPOINT outer_write')
    db.execute("UPDATE kv SET value=x'aa'")
    db.execute('SAVEPOINT nested_write')
    db.execute('INSERT INTO kv SELECT id+5000,value FROM kv WHERE id BETWEEN -30 AND 30')
    db.execute('ROLLBACK TO nested_write')
    db.execute('RELEASE nested_write')
    db.execute('ROLLBACK TO outer_write')
    db.execute('RELEASE outer_write')
    capture()
    db.close()
    db = sqlite3.connect(path)
    capture()
    db.close()
    return results


if __name__ == '__main__':
    if sys.argv[1] == '--child':
        print(json.dumps(child(sys.argv[2])))
        raise SystemExit
    library = Path(sys.argv[1]).resolve()
    with tempfile.TemporaryDirectory(prefix='ulmdb-btree-write-') as root:
        root = Path(root)
        runtime = root / 'runtime'
        runtime.mkdir()
        (runtime / 'libsqlite3.so.0').symlink_to(library)
        native = subprocess.run([sys.executable, __file__, '--child', str(root / 'native.db')],
                                text=True, capture_output=True, check=True)
        custom = subprocess.run([sys.executable, __file__, '--child', str(root / 'custom.db')],
                                text=True, capture_output=True,
                                env=dict(os.environ, LD_LIBRARY_PATH=str(runtime)))
        if custom.returncode:
            raise AssertionError((custom.returncode, custom.stdout, custom.stderr))
        assert json.loads(custom.stdout) == json.loads(native.stdout), 'native/custom write bytes differ'
        print('direct put: full bytes, mutation movement, transfer/reserve, blob, rollback and reopen: pass')
