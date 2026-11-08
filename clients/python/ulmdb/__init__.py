"""ULMDB byte transactions, ordered cursors and native IPv4/IPv6 indexes.

The database engine is loaded from libulmdb_c; see ULMDB_LIBRARY. Returned
values are independent Python bytes, including values read through cursors.
"""

import ctypes as C
import os
import threading
import weakref
from dataclasses import dataclass

from ._native import Value, input_value, load_library, output_value, text

__version__ = "0.1.0"
__all__ = ["Environment", "Transaction", "Database", "Cursor", "Error"]

_NOTFOUND = -30798
_RDONLY = 0x20000
_NOTLS = 0x200000
_CREATE = 0x40000
_NOOVERWRITE = 0x10
_NODUPDATA = 0x20
_KINDS = {"kv": 0, "duplicates": 1, "ipv4": 4, "ipv6": 6}


class Error(Exception):
    """A native ULMDB error, with its LMDB-compatible numeric ``code``."""

    def __init__(self, code, message):
        self.code = code
        super().__init__(message)


def _check(lib, code):
    if code:
        message = lib.mdb_strerror(code)
        raise Error(code, message.decode("utf-8", errors="replace") if message else str(code))


@dataclass(frozen=True)
class Database:
    """An environment-bound handle opened with ``Transaction.open_db``."""

    _environment: "Environment"
    _handle: int
    name: object
    kind: str


class Environment:
    """Open or create a database directory.

    ``readonly=True`` opens an existing environment and prevents writes.
    Close all transactions before closing the environment. Independent read
    transactions use separate reader slots; adapter processes coordinate access
    to the same environment directory.
    """

    def __init__(self, path, *, readonly=False, max_dbs=128, library=None):
        self._lib = load_library(library)
        self._handle = C.c_void_p()
        self._transactions = weakref.WeakSet()
        self._lock = threading.RLock()
        self.readonly = readonly
        if not isinstance(max_dbs, int) or not 1 <= max_dbs <= 4096:
            raise ValueError("max_dbs must be between 1 and 4096")
        path_bytes = os.fsencode(path)
        if b"\0" in path_bytes:
            raise ValueError("path contains NUL")
        _check(self._lib, self._lib.mdb_env_create(C.byref(self._handle)))
        try:
            _check(self._lib, self._lib.mdb_env_set_maxdbs(self._handle, max_dbs))
            _check(self._lib, self._lib.mdb_env_open(
                self._handle, path_bytes, _NOTLS | (_RDONLY if readonly else 0), 0o600))
        except BaseException:
            self._lib.mdb_env_close(self._handle)
            self._handle = C.c_void_p()
            raise

    def _require_open(self):
        if not self._handle:
            raise RuntimeError("environment is closed")

    def begin(self, *, write=False):
        """Start a snapshot; write contexts commit on success and abort on error."""
        with self._lock:
            self._require_open()
            if write and self.readonly:
                raise RuntimeError("environment is read-only")
            if write and any(tx._write and tx._handle for tx in self._transactions):
                raise RuntimeError("this environment already has an active writer")
            transaction = Transaction(self, write)
            self._transactions.add(transaction)
            return transaction

    def close(self):
        with self._lock:
            if not self._handle:
                return
            if self._transactions:
                raise RuntimeError("close or abort active transactions before the environment")
            self._lib.mdb_env_close(self._handle)
            self._handle = C.c_void_p()

    def __enter__(self):
        self._require_open()
        return self

    def __exit__(self, *exc):
        self.close()

    def __del__(self):
        try:
            self.close()
        except Exception:
            pass


class Transaction:
    """One explicit transaction. Use the same thread for all its operations."""

    def __init__(self, environment, write):
        self._environment = environment
        self._lib = environment._lib
        self._write = write
        self._thread = threading.get_ident()
        self._handle = C.c_void_p()
        self._cursors = weakref.WeakSet()
        _check(self._lib, self._lib.mdb_txn_begin(environment._handle, None,
                                              0 if write else _RDONLY,
                                              C.byref(self._handle)))

    def _require_open(self, write=False):
        if not self._handle:
            raise RuntimeError("transaction is closed")
        if self._thread != threading.get_ident():
            raise RuntimeError("transaction must be used on its creating thread")
        if write and not self._write:
            raise RuntimeError("transaction is read-only")

    def _db(self, database):
        if not isinstance(database, Database) or database._environment is not self._environment:
            raise ValueError("database belongs to a different environment")
        return database._handle

    def open_db(self, name=None, *, kind="kv", create=False):
        """Open a KV, sorted duplicates, IPv4 or IPv6 database in this transaction.

        ``name=None`` opens the unnamed KV database. Named database creation
        requires a write transaction and is rolled back when it aborts.
        """
        self._require_open(write=create)
        if kind not in _KINDS:
            raise ValueError("kind must be kv, duplicates, ipv4, or ipv6")
        if name is None and kind != "kv":
            raise ValueError("the unnamed database has kind kv")
        handle = C.c_uint()
        _check(self._lib, self._lib.ulmdb_dbi_open(
            self._handle, None if name is None else text(name), _KINDS[kind],
            _CREATE if create else 0, C.byref(handle)))
        return Database(self._environment, handle.value, name, kind)

    def get(self, database, key):
        """Return bytes, or None if the key is absent; duplicates return the first value."""
        self._require_open()
        key_value, backing = input_value(key)
        result = Value()
        code = self._lib.mdb_get(self._handle, self._db(database), C.byref(key_value), C.byref(result))
        if code == _NOTFOUND:
            return None
        _check(self._lib, code)
        return output_value(result)

    def put(self, database, key, value, *, overwrite=True, no_dup_data=False):
        """Store bytes; overwrite=False rejects an existing key with Error."""
        self._require_open(write=True)
        key_value, key_backing = input_value(key)
        data_value, data_backing = input_value(value)
        flags = (0 if overwrite else _NOOVERWRITE) | (_NODUPDATA if no_dup_data else 0)
        _check(self._lib, self._lib.mdb_put(self._handle, self._db(database),
                                          C.byref(key_value), C.byref(data_value), flags))

    def delete(self, database, key, value=None):
        """Remove a key (or one duplicate value), returning whether it existed."""
        self._require_open(write=True)
        key_value, key_backing = input_value(key)
        data_value, data_backing = input_value(value) if value is not None else (None, None)
        code = self._lib.mdb_del(self._handle, self._db(database), C.byref(key_value),
                                C.byref(data_value) if data_value is not None else None)
        if code == _NOTFOUND:
            return False
        _check(self._lib, code)
        return True

    def items(self, database):
        """Iterate ordered (key, value) bytes, including all duplicate values.

        The cursor closes on exhaustion or transaction end. For partial scans,
        use ``with transaction.items(db) as cursor: ...`` or ``cursor.close()``.
        """
        self._require_open()
        cursor = Cursor(self, self._db(database))
        self._cursors.add(cursor)
        return cursor

    def prefix_put(self, database, cidr, value):
        """Insert a CIDR in an IPv4 or IPv6 database."""
        self._require_open(write=True)
        data_value, backing = input_value(value)
        _check(self._lib, self._lib.ulmdb_prefix_put(
            self._handle, self._db(database), text(str(cidr)), C.byref(data_value)))

    def prefix_get(self, database, cidr):
        """Return an exact CIDR's value, or None."""
        self._require_open()
        value = Value()
        code = self._lib.ulmdb_prefix_get(self._handle, self._db(database), text(str(cidr)), C.byref(value))
        if code == _NOTFOUND:
            return None
        _check(self._lib, code)
        return output_value(value)

    def prefix_delete(self, database, cidr):
        """Delete an exact CIDR, returning whether it existed."""
        self._require_open(write=True)
        code = self._lib.ulmdb_prefix_delete(self._handle, self._db(database), text(str(cidr)))
        if code == _NOTFOUND:
            return False
        _check(self._lib, code)
        return True

    def longest_match(self, database, address):
        """Return (canonical CIDR string, bytes), or None for an unmatched address."""
        self._require_open()
        prefix, value = Value(), Value()
        code = self._lib.ulmdb_longest_match(self._handle, self._db(database), text(str(address)),
                                             C.byref(prefix), C.byref(value))
        if code == _NOTFOUND:
            return None
        _check(self._lib, code)
        return output_value(prefix).decode("ascii"), output_value(value)

    def _finish(self, commit):
        with self._environment._lock:
            if not self._handle:
                return
            self._require_open()
            for cursor in list(self._cursors):
                cursor.close()
            handle, self._handle = self._handle, C.c_void_p()
            try:
                if commit:
                    _check(self._lib, self._lib.mdb_txn_commit(handle))
                else:
                    self._lib.mdb_txn_abort(handle)
            finally:
                self._environment._transactions.discard(self)

    def commit(self):
        self._require_open()
        self._finish(True)

    def abort(self):
        self._finish(False)

    def __enter__(self):
        self._require_open()
        return self

    def __exit__(self, exc_type, exc, traceback):
        self._finish(exc_type is None and self._write)

    def __del__(self):
        try:
            self.abort()
        except Exception:
            pass


class Cursor:
    """A transaction-bound forward cursor yielding independent bytes pairs."""

    def __init__(self, transaction, database):
        self._transaction = transaction
        self._lib = transaction._lib
        self._handle = C.c_void_p()
        self._first = True
        _check(self._lib, self._lib.mdb_cursor_open(transaction._handle, database, C.byref(self._handle)))

    def __iter__(self):
        return self

    def __next__(self):
        if not self._handle:
            raise StopIteration
        self._transaction._require_open()
        key, value = Value(), Value()
        code = self._lib.mdb_cursor_get(self._handle, C.byref(key), C.byref(value),
                                        0 if self._first else 8)
        self._first = False
        if code == _NOTFOUND:
            self.close()
            raise StopIteration
        _check(self._lib, code)
        return output_value(key), output_value(value)

    def close(self):
        if self._handle:
            self._transaction._require_open()
            self._lib.mdb_cursor_close(self._handle)
            self._handle = C.c_void_p()
            self._transaction._cursors.discard(self)

    def __enter__(self):
        return self

    def __exit__(self, *exc):
        self.close()

    def __del__(self):
        try:
            self.close()
        except Exception:
            pass
