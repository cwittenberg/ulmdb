# ULMDB C++ client

A header-only C++17 facade over ULMDB's C ABI. It provides RAII environments,
transactions and cursors, binary key/value databases, sorted duplicates, and
native IPv4/IPv6 prefix indexes. It links to `libulmdb_c` and uses the C++ standard
library. Building it needs a C++17 compiler and CMake; no Python tooling is used.

From the repository root:

```sh
cargo build -p ulmdb-c --locked
cmake -S clients/cpp -B target/ulmdb-cpp \
  -DULMDB_C_LIBRARY="$PWD/target/debug/libulmdb_c.so"
cmake --build target/ulmdb-cpp
ctest --test-dir target/ulmdb-cpp --output-on-failure
./target/ulmdb-cpp/ulmdb_cpp_basic /tmp/new-ulmdb-cpp
./target/ulmdb-cpp/ulmdb_cpp_routes /tmp/new-ulmdb-cpp-routes
```

The examples create or open their database directories; parent directories must
exist. CMake also searches the repository's `target/debug` and `target/release`
directories for the C library. Use `add_subdirectory(clients/cpp)` and link
`ulmdb::cpp` from an existing CMake project. Set `ULMDB_CPP_BUILD_TESTS=OFF` and
`ULMDB_CPP_BUILD_EXAMPLES=OFF` when only the interface target is wanted.
`ULMDB_C_LIBRARY` can also name `libulmdb_c.a`; CMake includes the Linux system
libraries required by the Rust static archive.

```cpp
#include <ulmdb/ulmdb.hpp>
#include <iostream>

int main() {
    ulmdb::environment env("./example-db");
    auto write = env.begin_write();
    auto people = write.open_database("people", ulmdb::database_kind::kv, true);
    write.put(people, "alice", "Alice");
    write.commit();

    auto read = env.begin_read();
    if (auto value = read.get(people, "alice")) std::cout << *value << '\n';
    for (const auto& [key, value] : read.open_cursor(people))
        std::cout << key << " = " << value << '\n';
}
```

`environment_options` configures map capacity, database/reader limits, open flags
and read-only operation. A zero `map_size` adopts an existing environment's
capacity; new environments use the C adapter's default. Transactions and cursors
are move-only. A transaction commits only when `commit()` is called; destruction
and `abort()` roll back pending writes. Read transaction destruction releases its
snapshot. A nested writer is created with `write.nested()`; a live child blocks
other parent operations until the child commits or aborts. A child's commit
remains private until the outer writer commits.

Database handles can be copied and used in later transactions of the same
environment. `open_database(name, kind, true)` creates a missing database inside a
writer; `main_database()` selects the unnamed KV database. Creation rolled back
by a child or outer transaction invalidates every copied handle to that database,
including when a later creation reuses its C DBI slot. A database from another
environment is rejected.

`get()` and `prefix_get()` return `std::optional<std::string>`; `longest_match()`
returns an optional `{cidr, value}` result. Cursor methods return optional
`std::pair<std::string, std::string>` records. All results own their bytes and
survive cursor movement and transaction/environment closure. Keys and values
accept `std::string_view`, including embedded NUL bytes when the view supplies the
correct length. Database names, filesystem paths and textual addresses/prefixes
reject embedded NULs. `put()` returns false when no-overwrite or no-duplicate-data
options reject an existing record; `erase()` and `prefix_erase()` return false
when no record exists. Other C errors throw `ulmdb::error` with their numeric
`code()`.

Use `database_kind::duplicates` for sorted duplicate values. `open_cursor()`
supports ordered and reverse movement, exact/range seeks, duplicate seeks/counts,
insertion, replacement and deletion. Its range-based loop uses a single-pass input
iterator: advancing a copied iterator shares the cursor's position. Iterator
records own their bytes. Dereferencing an iterator returns its cached record,
valid until that iterator is advanced or destroyed; copy the record to retain it.
Ending a transaction or explicitly closing the cursor invalidates further
movement safely.

For native IP indexes, create databases with `database_kind::ipv4` or `ipv6` and
use `prefix_put`, `prefix_get`, `prefix_erase`, and `longest_match`. Prefixes must be
canonical CIDRs, for example `192.0.2.0/24` and `2001:db8::/32`.

```cpp
auto write = env.begin_write();
auto routes = write.open_database("routes6", ulmdb::database_kind::ipv6, true);
write.prefix_put(routes, "2001:db8::/32", "documentation");
write.commit();
auto read = env.begin_read();
auto match = read.longest_match(routes, "2001:db8::1");
```

Transactions, cursors and their iterators stay on their creating thread. Calls
from another thread throw `MDB_BAD_TXN`; do not concurrently mutate a wrapper
object. Move operations transfer ownership on the same thread. Destroying a live
transaction/cursor on another thread terminates the process instead of invoking
the thread-affine native cleanup incorrectly. End or close it on its owning
thread before transferring the ended wrapper elsewhere.

Active transactions, cursors and database handles retain the native environment.
Closing or destroying an environment facade therefore leaves its dependants safe.
Ending a transaction closes and invalidates its cursors before releasing the
native transaction. Aborting/destroying a parent also invalidates live child
wrappers and their cursors. Shared control blocks implement ownership; the public
classes form a Facade and cursors provide Iterator traversal. Headers are split
by responsibility and stay below the repository's 3,000-line source limit.

The tests exercise binary/empty values, snapshots, duplicate cursor mutations,
nested rollback, stale-handle reuse, wrong-environment/thread guards, move and
destruction order, read-only errors, IPv4/IPv6 lookup, and bidirectional exchange
with direct C ABI calls in the same native database directory. The [C adapter
guide](../c/README.md) describes storage and compatibility boundaries. This C++
client is a native API; existing LMDB C applications can use the separate
[LMDB adapter](../lmdb-ulmdb/README.md).
