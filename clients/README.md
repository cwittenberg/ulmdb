# ULMDB clients

These clients use the same ULMDB engine and storage format. They expose ordered
binary KV databases, sorted duplicate values, transactions and native IPv4/IPv6
prefix indexes. Each directory includes tests, examples and setup instructions.

| Directory | Package or library | Interface |
| --- | --- | --- |
| [c](c/README.md) | `ulmdb-c`; `libulmdb_c` | Common LMDB C declarations and `mdb_*` symbols, plus `ulmdb_*` IP extensions |
| [cpp](cpp/README.md) | C++17 headers; namespace `ulmdb` | RAII environments, transactions and cursors, owned binary results and IPv4/IPv6 operations |
| [rust](rust/README.md) | `ulmdb-rs`; import `ulmdb_rs` | Safe native Rust API, borrowed snapshot reads and the full engine API |
| [python](python/README.md) | `ulmdb`; import `ulmdb` | Python `ctypes` client with transaction contexts and owned byte results |
| [php](php/README.md) | `ulmdb/ulmdb`; namespace `Ulmdb` | PHP FFI client with binary strings, transaction callbacks and Docker-only tests |
| [node](node/README.md) | `ulmdb-js` | Synchronous Node.js N-API client, `Buffer` results and TypeScript declarations |
| [lmdb-ulmdb](lmdb-ulmdb/README.md) | `liblmdb` adapter | Existing LMDB applications, dynamic library replacement and snapshot migration |

The directory names are `rust` and `node`; their package names remain
`ulmdb-rs` and `ulmdb-js`.

The C, C++, Python, PHP and Node clients share the C library. Run all client tests and the
shared-directory interoperability test with `./clients/test.sh` from the
repository root. Individual commands are:

```sh
cargo build -p ulmdb-c --locked
cargo test -p ulmdb-rs --locked
cmake -S clients/cpp -B target/ulmdb-cpp -DULMDB_C_LIBRARY="$PWD/target/debug/libulmdb_c.so"
cmake --build target/ulmdb-cpp
ctest --test-dir target/ulmdb-cpp --output-on-failure
python3 clients/c/tests/compare.py --no-build
PYTHONPATH=clients/python python3 -m unittest discover -s clients/python/tests -v
npm --prefix clients/node run build
npm --prefix clients/node test
./clients/php/test-docker.sh
python3 clients/tests/interoperability.py
```

The C comparison needs the pinned LMDB source in the repository's comparison
cache, or `ULMDB_LMDB_SOURCE` pointing to that checkout. It checks the source
hashes before compiling the same C program against each engine. See the
[C compatibility scope](c/README.md) for what this test establishes and the
remaining differences. Python needs no runtime dependencies beyond the standard
library. The adapter package also needs the pinned LMDB 0.9 source cache;
`make -C clients/lmdb-ulmdb package ARGS='--fetch-legacy'` prepares a missing cache.
Node requires a C compiler and Node development headers; its client has
no npm runtime dependencies. PHP tests require Docker and build PHP plus FFI
inside the image; no host PHP installation is needed. The PHP suite also checks
Python/PHP persisted-data interoperability. Installed Python/Node/PHP packages
need the ULMDB shared library; set `ULMDB_LIBRARY` to its absolute path outside
the source checkout.
The interoperability test creates one environment with Rust, reads and extends
it with Python, reads it through the C ABI, extends it with Node and verifies
the final binary records and IPv4/IPv6 longest matches in Rust. Opens happen
sequentially; each client releases the environment before the next starts.

Run the examples:

```sh
cargo run -p ulmdb-rs --example basic -- /tmp/new-ulmdb-rust
cargo run -p ulmdb-rs --example routes -- /tmp/new-ulmdb-routes
PYTHONPATH=clients/python python3 clients/python/examples/quickstart.py
npm --prefix clients/node run example
```

Rust example paths must be absent. The Python and Node examples create temporary
databases. [C examples](c/README.md#tests-and-examples) demonstrate the standard byte API
and native IPv4/IPv6 prefix operations separately.

The [LMDB adapter](lmdb-ulmdb/README.md) automatically converts existing LMDB
snapshots to native sidecars while preserving calls, paths and original data.
Its launcher selects the replacement dynamic library for existing applications.
IP data stored as ordinary LMDB KV records must be
decoded into CIDR prefixes to use ULMDB's native IP indexes.

The native Rust environment retains its existing writer-ownership contract.
C, Python, PHP and Node adapter processes share environments through process leases
and independent immutable snapshots. Python, PHP and Node transactions stay on their creating thread. C writers
and ordinary readers also stay on their creating thread; readers opened with
`MDB_NOTLS` may move between threads when used sequentially. Rust uses the
native engine's type and lifetime contracts. Python, PHP and Node provide
new APIs; substituting them for unrelated LMDB language bindings requires
adapting application calls.

The engine remains experimental. Client tests demonstrate interface behavior;
they do not qualify durability, performance parity or complete LMDB compliance.
See the repository [README](../README.md) for engine status and measured benchmarks.
