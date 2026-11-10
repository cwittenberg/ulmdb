#!/usr/bin/env python3
"""Differential SQL movement and live-snapshot tests, one SQLite per process."""
import hashlib
import json
import os
from pathlib import Path
import subprocess
import sys
import tempfile


def child(path, custom):
    import sqlite3
    if custom:
        loaded = {line.split()[-1] for line in Path('/proc/self/maps').read_text().splitlines()
                  if 'libsqlite3' in line and line.split()[-1].startswith('/')}
        assert loaded == {str(Path(custom).resolve())}, loaded
    assert sqlite3.sqlite_version_info >= (3, 38)
    a = sqlite3.connect(path, isolation_level=None)
    a.execute('PRAGMA journal_mode=WAL')
    a.execute('CREATE TABLE kv(id INTEGER PRIMARY KEY, value BLOB NOT NULL)')
    keys = [-(1 << 63), -19, -7, -1, 0, 1, 4, 9, 19, (1 << 63) - 1]
    value = lambda key: hashlib.sha256(str(key).encode()).digest() * 8
    a.executemany('INSERT INTO kv VALUES(?,?)', [(key, value(key)) for key in keys])
    if custom:
        for operation in [lambda: a.serialize(), lambda: a.deserialize(b'not a SQLite image')]:
            try:
                operation()
            except sqlite3.DatabaseError:
                pass
            else:
                raise AssertionError('unsupported serialization API succeeded')
        assert a.execute('SELECT count(*) FROM kv').fetchone()[0] == len(keys)
    results = []
    a.execute('BEGIN')
    for iteration in range(3):
        for key in keys + [-20, -18, -2, 3, 20]:
            got = a.execute('SELECT id,hex(value) FROM kv WHERE id=?', (key,)).fetchall()
            results.append(got)
            for comparison in ['>=', '>', '<=', '<']:
                for direction in ['ASC', 'DESC']:
                    results.append(a.execute(f'SELECT id,hex(value) FROM kv WHERE id{comparison}? ORDER BY id {direction}', (key,)).fetchall())
    results.append(a.execute('SELECT id,hex(value) FROM kv WHERE id IN (19,-7,1,-7,NULL,3,-9223372036854775808) ORDER BY id DESC').fetchall())
    a.commit()
    # A current reader retains its exact value/schema epoch across external
    # commits. Ending it renews both, including cached prepared statements.
    b = sqlite3.connect(path, isolation_level=None)
    a.execute('BEGIN')
    old = a.execute('SELECT value FROM kv WHERE id=1').fetchone()[0]
    epoch = a.execute('PRAGMA data_version').fetchone()[0]
    b.execute("UPDATE kv SET value=x'aabbcc' WHERE id=1")
    b.execute('ALTER TABLE kv ADD COLUMN added TEXT DEFAULT \'fresh\'')
    assert a.execute('SELECT value FROM kv WHERE id=1').fetchone()[0] == old
    assert a.execute('PRAGMA data_version').fetchone()[0] == epoch
    assert len(a.execute('PRAGMA table_info(kv)').fetchall()) == 2
    a.commit()
    assert a.execute('SELECT value FROM kv WHERE id=1').fetchone()[0] == b'\xaa\xbb\xcc'
    assert len(a.execute('PRAGMA table_info(kv)').fetchall()) == 3
    assert a.execute('PRAGMA data_version').fetchone()[0] != epoch
    # A writer based on an older pinned read must refuse promotion.
    a.execute('BEGIN')
    a.execute('SELECT value FROM kv WHERE id=4').fetchone()
    b.execute("UPDATE kv SET value=x'ddee' WHERE id=4")
    try:
        a.execute("UPDATE kv SET value=x'00' WHERE id=4")
    except sqlite3.OperationalError as error:
        assert error.sqlite_errorcode & 255 == sqlite3.SQLITE_BUSY, error
    else:
        raise AssertionError('stale reader was promoted')
    a.rollback()
    assert a.execute('SELECT value FROM kv WHERE id=4').fetchone()[0] == b'\xdd\xee'
    # The initial row is pending at execute(): its SQLite result memory and
    # cursor position must survive promotion by another statement.
    observer = None
    if custom:
        import ctypes
        if os.environ.get('ULMDB_BTREE_NATIVE_TEST'):
            api = ctypes.CDLL(custom)
            class Info(ctypes.Structure):
                _fields_ = [('address', ctypes.c_void_p), ('map_size', ctypes.c_size_t),
                            ('last_page', ctypes.c_size_t), ('sequence', ctypes.c_size_t),
                            ('max_readers', ctypes.c_uint), ('readers', ctypes.c_uint)]
            api.mdb_env_create = api.ubt_env_create
            api.mdb_env_open = api.ubt_env_open
            api.mdb_env_close = api.ubt_env_close
            api.ubt_env_info.argtypes = [ctypes.c_void_p, ctypes.POINTER(Info)]
        else:
            api = ctypes.CDLL('libulmdb_c.so')
        api.mdb_env_create.argtypes = [ctypes.POINTER(ctypes.c_void_p)]
        api.mdb_env_open.argtypes = [ctypes.c_void_p, ctypes.c_char_p,
                                     ctypes.c_uint, ctypes.c_uint]
        api.mdb_env_close.argtypes = [ctypes.c_void_p]
        api.mdb_env_close.restype = None
        observer = ctypes.c_void_p()
        assert api.mdb_env_create(ctypes.byref(observer)) == 0
        native_backend = bool(os.environ.get('ULMDB_BTREE_NATIVE_TEST'))
        assert api.mdb_env_open(observer, os.fsencode(path),
                                0x20000 | 0x200000 | (0 if native_backend else 0x4000), 0) == 0
        if native_backend:
            def active_readers():
                info = Info()
                assert api.ubt_env_info(observer, ctypes.byref(info)) == 0
                return info.readers
        else:
            callback_type = ctypes.CFUNCTYPE(ctypes.c_int, ctypes.c_char_p, ctypes.c_void_p)
            api.mdb_reader_list.argtypes = [ctypes.c_void_p, callback_type, ctypes.c_void_p]
            def active_readers():
                records = []
                @callback_type
                def receive(line, _context):
                    records.append(line.decode().split())
                    return 0
                # This API holds the registry lock and excludes free slots.
                assert api.mdb_reader_list(observer, receive, None) == 0
                count = 0
                for record in records:
                    if record and record[0].isdigit():
                        assert len(record) == 3 and (record[2] == '-' or record[2].isdigit()), record
                        count += record[2] != '-'
                return count

    a.execute('BEGIN')
    pending = a.execute('SELECT id,hex(value) FROM kv WHERE id>=0 ORDER BY id')
    pending_second = a.execute('SELECT id,hex(value) FROM kv WHERE id<=9 ORDER BY id DESC')
    a.execute("UPDATE kv SET value=x'112233' WHERE id=9")
    results.append(pending.fetchall())
    pending.close()
    if custom:
        assert active_readers() == (2 if native_backend else 1), 'last pending row lost its actual native reader'
    results.append(pending_second.fetchone())
    if custom:
        assert active_readers() == (2 if native_backend else 1), 'remaining cursor lost its actual native reader'
    pending_second.close()
    if custom:
        assert active_readers() == (1 if native_backend else 0), 'closed cursors retained the retired native pin'
    a.rollback()
    if custom:
        assert active_readers() == 0, 'rollback retained an actual native epoch'
    if observer:
        api.mdb_env_close(observer)
    if custom:
        a.execute('BEGIN')
        a.execute('SELECT value FROM kv WHERE id=1').fetchone()
        pid = os.fork()
        if pid == 0:
            try:
                a.execute('SELECT * FROM kv WHERE 0').fetchall()
            except sqlite3.DatabaseError:
                os._exit(0)
            os._exit(1)
        _, status = os.waitpid(pid, 0)
        assert status == 0, 'forked held-reader cache was admitted'
        a.rollback()
    b.close()
    a.close()
    return results


if __name__ == '__main__':
    if len(sys.argv) > 1 and sys.argv[1] == '--child':
        print(json.dumps(child(sys.argv[2], os.environ.get('ULMDB_CUSTOM_SQLITE'))))
        raise SystemExit
    library = Path(sys.argv[1]).resolve()
    with tempfile.TemporaryDirectory(prefix='ulmdb-btree-runtime-') as root:
        root = Path(root)
        runtime = root / 'runtime'
        runtime.mkdir()
        (runtime / 'libsqlite3.so.0').symlink_to(library)
        base = os.environ.copy()
        native = subprocess.run([sys.executable, __file__, '--child', str(root / 'native.db')],
                                capture_output=True, text=True, env=base, check=True)
        env = dict(base, LD_LIBRARY_PATH=str(runtime), ULMDB_CUSTOM_SQLITE=str(library))
        if '--native' in sys.argv[2:]:
            env['ULMDB_BTREE_NATIVE_TEST'] = '1'
        result = subprocess.run([sys.executable, __file__, '--child', str(root / 'custom.db')],
                                capture_output=True, text=True, env=env)
        if result.returncode:
            raise AssertionError((result.returncode, result.stdout, result.stderr))
        assert json.loads(result.stdout) == json.loads(native.stdout), 'native/custom SQL movement differs'
        print('single-library Python ABI, exact/range movement, snapshot renewal and promotion: pass')
