#!/usr/bin/env python3
"""Repeated normal SQL reads cross the optional preparation threshold."""
import hashlib
import json
import os
from pathlib import Path
import subprocess
import sys
import tempfile


def child(path):
    import sqlite3
    custom = os.environ.get('ULMDB_CUSTOM_SQLITE')
    if custom:
        loaded = {line.split()[-1] for line in Path('/proc/self/maps').read_text().splitlines()
                  if 'libsqlite3' in line and line.split()[-1].startswith('/')}
        assert loaded == {str(Path(custom).resolve())}, loaded
    db = sqlite3.connect(path, isolation_level=None)
    db.execute('PRAGMA journal_mode=WAL')
    db.execute('CREATE TABLE kv(id INTEGER PRIMARY KEY,value BLOB NOT NULL)')
    keys = [-(1 << 63)] + list(range(200)) + [(1 << 63) - 1]
    values = {key: hashlib.sha256(str(key).encode()).digest() * 5 for key in keys}
    db.executemany('INSERT INTO kv VALUES(?,?)', values.items())
    peer = sqlite3.connect(path, isolation_level=None)
    out = []
    db.execute('BEGIN')
    version = db.execute('PRAGMA data_version').fetchone()[0]
    for index in range(256):
        key = keys[index % len(keys)]
        assert db.execute('SELECT value FROM kv WHERE id=?', (key,)).fetchone()[0] == values[key]
    peer.execute("UPDATE kv SET value=x'aabbcc' WHERE id=1")
    peer.execute('ALTER TABLE kv ADD COLUMN extra TEXT DEFAULT "new"')
    assert db.execute('SELECT value FROM kv WHERE id=1').fetchone()[0] == values[1]
    assert db.execute('PRAGMA data_version').fetchone()[0] == version
    assert len(db.execute('PRAGMA table_info(kv)').fetchall()) == 2
    for _ in range(70):
        assert not db.execute('SELECT value FROM kv WHERE id IN (-999,999,NULL)').fetchall()
    out.append(db.execute('SELECT id,hex(value) FROM kv WHERE id IN (199,1,0,1,NULL,-9223372036854775808,9223372036854775807) ORDER BY id DESC').fetchall())
    db.commit()
    values[1] = b'\xaa\xbb\xcc'
    assert len(db.execute('PRAGMA table_info(kv)').fetchall()) == 3
    assert db.execute('PRAGMA data_version').fetchone()[0] != version
    db.execute('BEGIN')
    for _ in range(80):
        assert db.execute('SELECT value FROM kv WHERE id=1').fetchone()[0] == values[1]
    # A pending row may refer to the actual indexed read through promotion.
    pending = db.execute('SELECT value FROM kv WHERE id=1')
    db.execute("UPDATE kv SET value=x'0102' WHERE id=2")
    assert pending.fetchone()[0] == values[1]
    pending.close()
    db.rollback()
    out.append(db.execute('SELECT id,hex(value),extra FROM kv ORDER BY id').fetchall())
    db.close()
    peer.close()
    db = sqlite3.connect(path)
    assert dict(db.execute('SELECT id,value FROM kv')) == values
    db.close()
    return out


if __name__ == '__main__':
    if sys.argv[1] == '--child':
        print(json.dumps(child(sys.argv[2])))
        raise SystemExit
    library = Path(sys.argv[1]).resolve()
    with tempfile.TemporaryDirectory(prefix='ulmdb-btree-read-hash-') as directory:
        root = Path(directory)
        runtime = root / 'runtime'
        runtime.mkdir()
        (runtime / 'libsqlite3.so.0').symlink_to(library)
        native = subprocess.run([sys.executable, __file__, '--child', str(root / 'native.db')],
                                text=True, capture_output=True, check=True)
        env = dict(os.environ, LD_LIBRARY_PATH=str(runtime), ULMDB_CUSTOM_SQLITE=str(library))
        custom = subprocess.run([sys.executable, __file__, '--child', str(root / 'custom.db')],
                                env=env, text=True, capture_output=True)
        if custom.returncode:
            raise AssertionError((custom.returncode, custom.stdout, custom.stderr))
        assert json.loads(custom.stdout) == json.loads(native.stdout)
        print('optional read hash: repeated point/IN, signed keys, peer commit, promotion and reopen: pass')
