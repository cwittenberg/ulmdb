#!/usr/bin/env python3
"""Root savepoints keep the same native writer reservation and SQL baseline."""
import json
import os
from pathlib import Path
import subprocess
import sys
import tempfile


def child(path, custom):
    import sqlite3
    fresh = sqlite3.connect(str(path) + '.fresh', isolation_level=None)
    fresh.execute('SAVEPOINT first_root')
    fresh.execute('CREATE TABLE discarded(id INTEGER PRIMARY KEY)')
    fresh.execute('ROLLBACK TO first_root')
    fresh.execute('CREATE TABLE kept(id INTEGER PRIMARY KEY)')
    fresh.execute('INSERT INTO kept VALUES(41)')
    fresh.execute('RELEASE first_root')
    assert fresh.execute("SELECT name FROM sqlite_schema WHERE type='table'").fetchall() == [('kept',)]
    fresh.close()
    fresh = sqlite3.connect(str(path) + '.fresh')
    assert fresh.execute('SELECT * FROM kept').fetchall() == [(41,)]
    fresh.close()
    a = sqlite3.connect(path, isolation_level=None)
    a.execute('PRAGMA journal_mode=WAL')
    a.execute('CREATE TABLE kv(id INTEGER PRIMARY KEY, value TEXT NOT NULL)')
    a.execute("INSERT INTO kv VALUES(1,'base')")
    b = sqlite3.connect(path, isolation_level=None, timeout=0)
    results = []

    observer = None
    if custom and os.environ.get('ULMDB_BTREE_NATIVE_TEST'):
        import ctypes
        api = ctypes.CDLL(os.environ['ULMDB_ROOT_LIBRARY'])
        api.ubt_env_create.argtypes = [ctypes.POINTER(ctypes.c_void_p)]
        api.ubt_env_open.argtypes = [ctypes.c_void_p, ctypes.c_char_p, ctypes.c_uint, ctypes.c_uint]
        api.ubt_env_close.argtypes = [ctypes.c_void_p]
        api.ubt_env_close.restype = None
        api.ubt_txn_begin.argtypes = [ctypes.c_void_p, ctypes.c_void_p, ctypes.c_uint, ctypes.POINTER(ctypes.c_void_p)]
        api.ubt_txn_abort.argtypes = [ctypes.c_void_p]
        api.ubt_txn_abort.restype = None
        api.ubt_txn_base_checkpoint.argtypes = [ctypes.c_void_p, ctypes.c_void_p]
        observer = ctypes.c_void_p()
        assert api.ubt_env_create(ctypes.byref(observer)) == 0
        assert api.ubt_env_open(observer, os.fsencode(path), 0x200000, 0) == 0

    def reservation():
        if not custom:
            return None
        if observer:
            writer = ctypes.c_void_p()
            assert api.ubt_txn_begin(observer, None, 0, ctypes.byref(writer)) == 16
            assert not writer.value, 'busy writer unexpectedly allocated a handle'
            reader = ctypes.c_void_p()
            assert api.ubt_txn_begin(observer, None, 0x20000, ctypes.byref(reader)) == 0
            checkpoint = (ctypes.c_ubyte * 64)()
            assert api.ubt_txn_base_checkpoint(reader, checkpoint) == 0
            api.ubt_txn_abort(reader)
            return bytes(checkpoint)
        import fcntl
        lease = Path(str(path) + '.ulmdb/LMDB-WRITER')
        stat = lease.stat()
        with lease.open('rb') as file:
            try:
                fcntl.flock(file, fcntl.LOCK_EX | fcntl.LOCK_NB)
            except BlockingIOError:
                pass
            else:
                raise AssertionError('native writer reservation was released')
            record = file.read()
        return stat.st_dev, stat.st_ino, record

    def other_writer_busy():
        try:
            b.execute('BEGIN IMMEDIATE')
        except sqlite3.OperationalError as error:
            assert error.sqlite_errorcode & 255 == sqlite3.SQLITE_BUSY, error
        else:
            raise AssertionError('other connection entered the reserved writer')

    a.execute('SAVEPOINT root')
    a.execute("UPDATE kv SET value='discard' WHERE id=1")
    before = reservation()
    other_writer_busy()
    a.execute('CREATE TABLE transient(id INTEGER PRIMARY KEY)')
    a.execute('ALTER TABLE kv ADD COLUMN discarded TEXT')
    a.execute('SAVEPOINT inner')
    a.execute("INSERT INTO kv(id,value) VALUES(2,'discard')")
    a.execute('ROLLBACK TO root')
    assert a.in_transaction
    assert reservation() == before
    other_writer_busy()
    assert a.execute("SELECT count(*) FROM sqlite_schema WHERE name='transient'").fetchone()[0] == 0
    assert len(a.execute('PRAGMA table_info(kv)').fetchall()) == 2
    results.append(a.execute('SELECT * FROM kv ORDER BY id').fetchall())
    a.execute("INSERT INTO kv VALUES(2,'second discard')")
    a.execute('ROLLBACK TO root')
    assert reservation() == before
    results.append(a.execute('SELECT * FROM kv ORDER BY id').fetchall())
    # A statement constraint rollback must preserve the root child and work
    # performed earlier in it, then root rollback must remove all that work.
    a.execute("INSERT INTO kv VALUES(3,'survive statement')")
    try:
        a.execute("INSERT INTO kv VALUES(4,'statement discard'),(3,'duplicate')")
    except sqlite3.IntegrityError:
        pass
    else:
        raise AssertionError('duplicate primary key accepted')
    results.append(a.execute('SELECT * FROM kv ORDER BY id').fetchall())
    a.execute('ROLLBACK TO root')
    assert reservation() == before
    a.execute("INSERT INTO kv VALUES(5,'committed')")
    a.execute('RELEASE root')
    assert not a.in_transaction
    results.append(b.execute('SELECT * FROM kv ORDER BY id').fetchall())
    if observer:
        writer = ctypes.c_void_p()
        assert api.ubt_txn_begin(observer, None, 0, ctypes.byref(writer)) == 0
        api.ubt_txn_abort(writer)
    b.execute("INSERT INTO kv VALUES(6,'next writer')")
    a.execute('SAVEPOINT constraint_root')
    a.execute("UPDATE kv SET value='discard by conflict' WHERE id=1")
    try:
        a.execute("INSERT OR ROLLBACK INTO kv VALUES(1,'duplicate')")
    except sqlite3.IntegrityError:
        pass
    else:
        raise AssertionError('INSERT OR ROLLBACK accepted a duplicate')
    assert not a.in_transaction
    assert a.execute('SELECT value FROM kv WHERE id=1').fetchone()[0] == 'base'
    b.execute("INSERT INTO kv VALUES(8,'writer after conflict')")

    # Read-only ROLLBACK TO keeps its original immutable read snapshot.
    a.execute('SAVEPOINT read_root')
    assert a.execute('SELECT value FROM kv WHERE id=1').fetchone()[0] == 'base'
    b.execute("UPDATE kv SET value='new peer value' WHERE id=1")
    a.execute('ROLLBACK TO read_root')
    assert a.execute('SELECT value FROM kv WHERE id=1').fetchone()[0] == 'base'
    a.execute('RELEASE read_root')
    assert a.execute('SELECT value FROM kv WHERE id=1').fetchone()[0] == 'new peer value'
    a.close()
    b.close()
    if observer:
        api.ubt_env_close(observer)
    reopened = sqlite3.connect(path)
    results.append(reopened.execute('SELECT * FROM kv ORDER BY id').fetchall())
    reopened.close()
    return results


if __name__ == '__main__':
    if sys.argv[1] == '--child':
        print(json.dumps(child(sys.argv[2], bool(os.environ.get('ULMDB_ROOT_TEST')))))
        raise SystemExit
    library = Path(sys.argv[1]).resolve()
    with tempfile.TemporaryDirectory(prefix='ulmdb-btree-root-') as root:
        root = Path(root)
        runtime = root / 'runtime'
        runtime.mkdir()
        (runtime / 'libsqlite3.so.0').symlink_to(library)
        native = subprocess.run([sys.executable, __file__, '--child', str(root / 'native.db')],
                                text=True, capture_output=True, check=True)
        env = dict(os.environ, LD_LIBRARY_PATH=str(runtime), ULMDB_ROOT_TEST='1',
                   ULMDB_ROOT_LIBRARY=str(library))
        if '--native' in sys.argv[2:]:
            env['ULMDB_BTREE_NATIVE_TEST'] = '1'
        custom = subprocess.run([sys.executable, __file__, '--child', str(root / 'custom.db')],
                                env=env, text=True, capture_output=True)
        if custom.returncode:
            raise AssertionError((custom.returncode, custom.stdout, custom.stderr))
        assert json.loads(custom.stdout) == json.loads(native.stdout)
        print('root rollback, repeated rollback, DDL/statement rollback, retained writer/base and reopen: pass')
