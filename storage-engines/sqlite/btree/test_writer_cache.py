#!/usr/bin/env python3
"""Native-writer cookie/version behavior against ordinary SQLite."""
import json
import os
from pathlib import Path
import subprocess
import sys
import tempfile


def child(path):
    import sqlite3
    db = sqlite3.connect(path, isolation_level=None)
    db.execute('CREATE TABLE kv(id INTEGER PRIMARY KEY, value TEXT)')
    out = []
    cookie = lambda: db.execute('PRAGMA schema_version').fetchone()[0]
    version = lambda: db.execute('PRAGMA data_version').fetchone()[0]
    original_version = version()
    db.execute('BEGIN IMMEDIATE')
    for key in range(100):
        db.execute('INSERT INTO kv VALUES(?,?)', (key, str(key)))
    original_cookie = cookie()
    db.execute('ALTER TABLE kv ADD COLUMN kept TEXT DEFAULT "kept"')
    assert cookie() == original_cookie + 1
    out.append(db.execute('SELECT kept FROM kv WHERE id=?', (4,)).fetchone())
    db.execute('SAVEPOINT ddl')
    db.execute('ALTER TABLE kv ADD COLUMN discarded TEXT DEFAULT "gone"')
    db.execute('CREATE TABLE discarded(id INTEGER PRIMARY KEY)')
    db.execute('SAVEPOINT deeper')
    db.execute('PRAGMA schema_version=500')
    assert cookie() == 500
    db.execute('ROLLBACK TO ddl')
    assert cookie() == original_cookie + 1
    out.append([column[1] for column in db.execute('PRAGMA table_info(kv)')])
    assert not db.execute("SELECT name FROM sqlite_schema WHERE name='discarded'").fetchall()
    db.execute('RELEASE ddl')
    db.execute('SAVEPOINT release_ddl')
    db.execute('ALTER TABLE kv ADD COLUMN merged TEXT DEFAULT "merged"')
    merged_cookie = cookie()
    db.execute('RELEASE release_ddl')
    assert cookie() == merged_cookie
    out.append(db.execute('SELECT merged FROM kv WHERE id=?', (4,)).fetchone())
    try:
        db.execute("INSERT INTO kv(id,value) VALUES(200,'discard'),(4,'duplicate')")
    except sqlite3.IntegrityError:
        pass
    else:
        raise AssertionError('duplicate accepted')
    assert db.execute('SELECT count(*) FROM kv WHERE id=200').fetchone()[0] == 0
    assert cookie() == merged_cookie
    db.commit()
    assert version() == original_version
    # SQL's root savepoint replaces its child while retaining the same writer.
    db.execute('SAVEPOINT root')
    root_cookie = cookie()
    db.execute('ALTER TABLE kv ADD COLUMN rolled_back INTEGER')
    db.execute('PRAGMA schema_version=700')
    assert cookie() == 700
    db.execute('ROLLBACK TO root')
    assert cookie() == root_cookie
    db.execute("UPDATE kv SET value='root-kept' WHERE id=4")
    db.execute('RELEASE root')
    # A fresh writer must read its actual new base, not the prior cache.
    peer = sqlite3.connect(path, isolation_level=None)
    peer.execute('ALTER TABLE kv ADD COLUMN external INTEGER DEFAULT 17')
    changed_version = version()
    assert changed_version != original_version
    db.execute('BEGIN IMMEDIATE')
    out.append(db.execute('SELECT external FROM kv WHERE id=?', (4,)).fetchone())
    db.execute("UPDATE kv SET value='discard full' WHERE id=4")
    db.execute('PRAGMA schema_version=900')
    db.rollback()
    out.append(db.execute('SELECT value,kept,merged,external FROM kv WHERE id=4').fetchone())
    assert cookie() != 900
    peer.close()
    db.close()
    db = sqlite3.connect(path)
    out.append(db.execute('SELECT id,value FROM kv ORDER BY id').fetchall())
    out.append([column[1] for column in db.execute('PRAGMA table_info(kv)')])
    db.close()
    return out


if __name__ == '__main__':
    if sys.argv[1] == '--child':
        print(json.dumps(child(sys.argv[2])))
        raise SystemExit
    library = Path(sys.argv[1]).resolve()
    with tempfile.TemporaryDirectory(prefix='ulmdb-btree-writer-cache-sql-') as root:
        root = Path(root)
        runtime = root / 'runtime'
        runtime.mkdir()
        (runtime / 'libsqlite3.so.0').symlink_to(library)
        native = subprocess.run([sys.executable, __file__, '--child', str(root / 'ordinary.db')],
                                text=True, capture_output=True, check=True)
        custom = subprocess.run([sys.executable, __file__, '--child', str(root / 'native.db')],
                                text=True, capture_output=True,
                                env=dict(os.environ, LD_LIBRARY_PATH=str(runtime)))
        assert custom.returncode == 0, (custom.returncode, custom.stdout, custom.stderr)
        assert json.loads(native.stdout) == json.loads(custom.stdout)
        print('writer metadata: own DDL, cookies, nested/root rollback, release, versions and reopen: pass')
