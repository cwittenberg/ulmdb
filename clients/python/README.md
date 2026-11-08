# ULMDB Python client

Python 3.9+ client using `ctypes` and `libulmdb_c`.

## Build

```sh
# From the repository root, with a Python virtual environment active:
cargo build -p ulmdb-c --locked
python3 -m pip install ./clients/python
export ULMDB_LIBRARY="$PWD/target/debug/libulmdb_c.so"
```

- macOS: use `libulmdb_c.dylib`.
- `ULMDB_LIBRARY` must name the shared library's absolute path.

## Use

```python
from ulmdb import Environment

with Environment("./example-db") as env:
    with env.begin(write=True) as txn:
        people = txn.open_db("people", create=True)
        txn.put(people, b"alice", b"Alice")
    with env.begin() as txn:
        print(txn.get(people, b"alice"))  # b'Alice'
```

- Write contexts commit on success and abort on exceptions.
- Keys/values are bytes; missing reads return `None`.
- Keep transactions on their creating thread.
- New API; for existing `import lmdb` applications use the [LMDB adapter](../lmdb-ulmdb/README.md).
- [IPv4/IPv6 example](examples/quickstart.py) · [API source](ulmdb/__init__.py) · [C API and limits](../c/README.md)

## Test

```sh
PYTHONPATH=clients/python python3 -m unittest discover -s clients/python/tests -v
PYTHONPATH=clients/python python3 clients/python/examples/quickstart.py
```

- Optional: [`python-lmdb` compatibility test](compatibility/test_python_lmdb.py).
