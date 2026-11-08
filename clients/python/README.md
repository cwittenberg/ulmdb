# ULMDB Python client

Python 3.9+ client using the ULMDB C shared library through the standard library's
`ctypes`. It supports explicit transactions, binary keys and values, named KV and
sorted duplicate databases, ordered cursors, and native IPv4/IPv6 prefix indexes.
This is a new Python API; it does not replace the `python-lmdb` package API.

From the repository root:

```sh
cargo build -p ulmdb-c
PYTHONPATH=clients/python python3 -m unittest discover -s clients/python/tests -v
PYTHONPATH=clients/python python3 clients/python/examples/quickstart.py
```

To install, run `python3 -m pip install ./clients/python`. Set `ULMDB_LIBRARY` to
the absolute path of `target/debug/libulmdb_c.so` (Linux),
`target/debug/libulmdb_c.dylib` (macOS).
The client also checks repository debug/release build directories and the system
library loader. Installing the Python package does not compile the Rust engine.
Use `Environment(path, library="/absolute/path/to/libulmdb_c.so")` for an explicit
library selection.

```python
from ulmdb import Environment

with Environment("./example-db") as env:
    with env.begin(write=True) as txn:
        people = txn.open_db("people", create=True)
        routes = txn.open_db("routes6", kind="ipv6", create=True)
        txn.put(people, b"alice", b"Alice")
        txn.prefix_put(routes, "2001:db8::/32", b"documentation")
    with env.begin() as txn:
        assert txn.get(people, b"alice") == b"Alice"
        assert txn.longest_match(routes, "2001:db8::1") == (
            "2001:db8::/32", b"documentation"
        )
        for key, value in txn.items(people):
            print(key, value)
```

Write transaction contexts commit on success and abort on exceptions. Read contexts
release their snapshot on exit. Explicit `commit()` and idempotent `abort()` are
also available. Open a database with `kind="kv"` (the default), `"duplicates"`,
`"ipv4"`, or `"ipv6"`; creation requires a writer and `create=True`. Use
`txn.open_db()` for the unnamed KV database. Handles belong to their environment;
a database created by an aborted transaction must be reopened after creation.

`get()` and `prefix_get()` return `bytes` or `None` when absent. `longest_match()`
returns `(canonical_cidr, bytes)` or `None`. `delete()` and `prefix_delete()` return
whether the entry existed. `put(..., overwrite=False)` rejects existing keys;
`put(..., no_dup_data=True)` rejects existing duplicate key/value pairs. Other
native errors raise `ulmdb.Error` with the LMDB-compatible integer `code`.

Keys and values accept bytes and buffer objects. Text must be encoded explicitly.
Returned bytes own their storage and survive transaction close. Duplicate
databases iterate every sorted key/value pair; `get()` returns the first value
and `delete(db, key, value)` removes one pair. Cursors close at exhaustion or
transaction end. For an early exit, use
`with txn.items(db) as cursor:` or call `cursor.close()`.

Independent read transactions can overlap on the same thread; the client selects
`MDB_NOTLS` so each transaction owns its reader slot. Use each transaction on its
creating thread. Close transactions before closing the
environment. `Environment(..., readonly=True)` opens an existing database and
prevents writes. Independent adapter processes can share an environment: readers
pin committed snapshots and process leases serialize writers. Files use ULMDB's storage format and
cannot be opened directly as LMDB data files. The engine remains experimental;
see the repository's status and the C client's compatibility notes.

The optional [public `python-lmdb` compatibility test](compatibility/test_python_lmdb.py)
checks an existing Python LMDB application against both engines. Build the
[native adapter package](../lmdb-ulmdb/README.md) first, and supply a Python
interpreter whose `lmdb` extension already links to a shared LMDB library:

```sh
python3 clients/python/compatibility/test_python_lmdb.py \
  --python /path/to/python-with-shared-lmdb \
  --reference-lib /path/to/original-lmdb/lib
```

This test preserves the application and extension binaries while checking
automatic migration, binary/empty values, borrowed buffers, duplicates, integer
keys, fixed duplicates, cursors, snapshots and persisted updates. It does not
download or install dependencies.
