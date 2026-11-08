# ULMDB C client

`libulmdb_c` · LMDB `mdb_*` API · native IPv4/IPv6 extensions.

## Build

```sh
# From the repository root; requires Cargo and a C compiler
cargo build -p ulmdb-c --locked
```

- Headers: [lmdb.h](include/lmdb.h), [ulmdb.h](include/ulmdb.h).
- Library: `target/debug/libulmdb_c.so` (Linux), `.dylib` (macOS), `.a` (static).
- Existing LMDB applications: [library replacement and migration](../lmdb-ulmdb/README.md).
- Optional encryption: [OpenSSL module](crypto/README.md).

## Use

```sh
# Compile and run the byte-key/value example
cc -std=c11 -Wall -Wextra -Werror -I clients/c/include \
  clients/c/examples/basic.c -L target/debug -lulmdb_c \
  -Wl,-rpath,"$PWD/target/debug" -o /tmp/ulmdb-c-basic
/tmp/ulmdb-c-basic /tmp/example-ulmdb-c

# Compile and run the IPv4/IPv6 example
cc -std=c11 -Wall -Wextra -Werror -I clients/c/include \
  clients/c/examples/routes.c -L target/debug -lulmdb_c \
  -Wl,-rpath,"$PWD/target/debug" -o /tmp/ulmdb-c-routes
/tmp/ulmdb-c-routes /tmp/example-ulmdb-routes
```

- Source: [put/get/cursors](examples/basic.c) · [IP prefixes](examples/routes.c).
- Check every return code; `mdb_strerror()` formats errors.
- Commit writes with `mdb_txn_commit()`; abort unused transactions with `mdb_txn_abort()`.
- `MDB_val` results borrow the transaction; copy before update, reset/renew or end. Never free them.
- Writers and ordinary readers stay on their creating thread; `MDB_NOTLS` readers may move sequentially.
- Parent directories must exist. Native files use ULMDB's storage format.
- [Compatibility and resource limits](../lmdb-ulmdb/PARITY_GAPS.md)

## Test

```sh
cargo test -p ulmdb-c --locked
python3 clients/c/tests/compare.py --no-build
```

- Comparison requires Python 3.11+, a C compiler and the vendored pinned LMDB sources.
- Alternate pinned sources: `ULMDB_LMDB_SOURCE=/path/to/lmdb`.
- [Compatibility results](../lmdb-ulmdb/conformance/REPORT.md) · [All client tests](../test.sh)
