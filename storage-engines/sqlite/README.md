# SQLite optional ULMDB tables

Two optional SQLite virtual-table modules are available. SQLite still parses,
plans and executes SQL; the selected table's records live in ULMDB. Ordinary
SQLite tables continue using SQLite's existing storage. Neither module changes
ULMDB core or SQLite source.

| Module | Extension | Ownership and storage |
| --- | --- | --- |
| `ulmdb` | `ulmdb_sqlite.so` | Cooperating file clients through the existing ULMDB C ABI |
| `ulmdb_owned` | `ulmdb_sqlite_owned.so` | One process owns a native snapshot-log environment; connections in that process share it |

Build from the repository root (Linux, C compiler, SQLite 3.38+ development headers,
and the repository's Rust toolchain):

```sh
cargo build --release --locked -p ulmdb-c
make -C storage-engines/sqlite
```

The library links to `target/release/libulmdb_c.so` with its absolute build path.
To install elsewhere, rebuild with `ULMDB_LIBDIR=/absolute/library/directory`,
and deploy that library too. Set `CPPFLAGS` for nonstandard SQLite headers.
The parent directory of the ULMDB environment must exist.

```sql
.load ./storage-engines/sqlite/ulmdb_sqlite
CREATE VIRTUAL TABLE cache USING ulmdb('/absolute/path/cache.ulmdb');
BEGIN;
INSERT INTO cache(id,value) VALUES (42, CAST('hello' AS BLOB));
COMMIT;
SELECT id, value FROM cache WHERE id=42;
UPDATE cache SET value=CAST('updated' AS BLOB) WHERE id=42;
DELETE FROM cache WHERE id=42;
```

Build the owned module separately on Linux. It compiles the small native Rust
bridge in `native/` and statically links it into the loadable extension:

```sh
make -C storage-engines/sqlite owned
```

The public owned build selects the bridge's `versioned` feature; the private
Cargo default and ULMDB core defaults remain unchanged. `make owned` runs Cargo
each time so feature and dependency changes are checked. The equivalent direct
native build is:

```sh
cargo build --release --locked --features versioned \
  --manifest-path storage-engines/sqlite/native/Cargo.toml \
  --target-dir target/sqlite-native-owned-versioned
```

An explicit `ULMDB_NATIVE_LIB=/absolute/libulmdb_sqlite_native.a` selects a
prebuilt library and skips Cargo entirely, for example in a C-only runtime
container. `CFLAGS` selects portable compiler optimization options while the
Makefile keeps its required C11, warning and shared-library flags.

```sql
.load ./storage-engines/sqlite/ulmdb_sqlite_owned
CREATE VIRTUAL TABLE cache USING ulmdb_owned('/absolute/path/owned-cache');
INSERT INTO cache VALUES(42, CAST('hello' AS BLOB));
SELECT id,value FROM cache WHERE id=42;
```

Use a **new, dedicated directory** for the owned module. Its snapshot-log format
differs from the cooperative module and PostgreSQL adapter; the same directory
cannot be opened interchangeably. The native owner admits the store when it
opens, retains ULMDB's exclusive process lock, and exposes fresh native snapshots
through its shared in-process owner. A second process is refused while that
owner remains open. Connections in the owning process see committed changes
through the same owner; old cursors retain their earlier immutable snapshot.
This ownership model avoids repeating cooperative file admission per lookup.
It does not disable admission checks, locks, or synchronous commit.
The owning connection may reuse one empty cursor header and one empty reader
carrier. Closing a query drops its snapshot, borrowed view and owner reference
before reuse; a later query captures a fresh snapshot. Cached allocation headers
hold no rows, pins, writer reservation or file lease, and closing the connection
discards them. Sequential queries may rebind an empty carrier on another thread;
active native transaction and scan handles retain their creating-thread rule.

Owned write transactions stage a bounded key delta and savepoint undo history,
then apply it in one native synchronous transaction at `xSync`. The defaults
allow 65,536 distinct pending keys and 64 MiB combined staging/undo storage,
including metadata. The native index and journal capacity settings are 64 MiB
each; a single native transaction has a slightly smaller payload bound to leave
room for its metadata. Each owned blob is limited to 16 MiB, and the selected
versioned profile allocates 32 MiB to its value pool and has at most 65,536 live
key slots. Retained snapshots also consume capacity. Native admission can
refuse COMMIT even when SQL staging succeeded; exceeding a limit raises an error.
Close all connections before accessing or moving the store
from another process. The extension remains loaded until process exit for Rust
thread-local cleanup safety.
The owned extension exports its SQLite entry point and private `usql_*` ABI;
internal Rust symbols remain local to the plugin. This isolates it from other
loaded libraries and leaves ownership, fork guards and result lifetimes intact.

The fixed schema is `id INTEGER PRIMARY KEY, value BLOB NOT NULL`; `rowid` equals
`id`. Supply an explicit signed 64-bit integer key. Empty blobs are supported.
Change a key with `UPDATE ... SET id=...`; direct rowid changes are rejected.
Equality and ascending lower bounds on `id`/`rowid` use ULMDB's ordered index;
SQLite rechecks comparisons and applies remaining predicates. Upper bounds
currently use SQLite filtering rather than an early stop in the adapter.
Only actual INTEGER lower-bound arguments use a seek. Other argument types
use a full ordered scan and SQLite's residual comparison, so numeric text,
real values, NULL and blobs retain SQLite's comparison semantics.

For keyed batches, use ordinary SQL `IN` queries:

```sql
SELECT id, value FROM cache WHERE id IN (42, 7, 100, 42) ORDER BY id;
```

SQLite 3.38+ passes the entire `IN` set to one `xFilter` call through
`sqlite3_vtab_in`. The adapter sorts and deduplicates signed keys, then probes
with `mdb_get` under one fresh ULMDB read snapshot for the batch. Point equality
also uses `mdb_get` directly. Returned bytes are copied before releasing that
snapshot in the cooperative module. The owned module instead keeps the cheap
snapshot with the cursor and resolves the sorted key list as rows are requested;
SQLite copies borrowed bytes when producing each column. It does not allocate
a second materialized result set. No snapshot is cached across SQL statements.
The cooperative module leaves the residual `IN` predicate to SQLite. The owned
module uses SQLite's numeric-affinity parser and accepts only exact signed
integer matches, including integral numeric text and reals. This lets SQLite
omit duplicate equality/`IN` checks; NULL, blobs, nonnumeric text, fractions and
out-of-range numbers cannot match integer keys. Remaining predicates still run
in SQLite. Monotonically ordered key sets skip an additional sort.
Here, exact matching means after SQLite's own numeric-affinity conversion;
numeric text may round to a representable real first. At the REAL(-2^63)
boundary, the owned module follows ordinary SQLite integer comparison.
SQLite 3.40.1 and 3.45.1 indexed-rowid seeks reject that real even though their
ordinary comparison accepts it. The tests preserve this observed discrepancy;
the module does not claim identical indexed-seek behavior at that boundary.
`EXPLAIN QUERY PLAN` identifies the route as `INDEX 3:in-batch`. Batch execution
is bounded to 65,536 candidate keys and 64 MiB of blob data (plus bounded
key/row metadata); larger batches raise `SQLITE_TOOBIG`. Split larger requests
into SQL batches. This amortizes snapshot setup across keys while preserving a
consistent view within each batch; it does not make separate statements share
a read snapshot.

Transactions use a single synchronous ULMDB writer. `BEGIN`/`COMMIT`/`ROLLBACK`,
statement rollback, and nested `SAVEPOINT`/`ROLLBACK TO`/`RELEASE` map to ULMDB
transactions and savepoints. Reads inside a write transaction see its changes.
The adapter commits ULMDB in SQLite's `xSync` callback, because SQLite ignores
errors returned from `xCommit`. This lets ULMDB commit and durability failures
reach the SQL caller. **A successful `xSync` is the external commit boundary:
a later SQLite rollback cannot undo that ULMDB commit.** Use a dedicated
connection and transactions that write only this ULMDB table. Other writable
virtual tables, native table writes, commit hooks and SQLite catalog or pager
errors after `xSync` can cause a failed SQL transaction whose ULMDB changes have
already committed. There is no cross-store atomic commit or recovery coordinator.
Read-only scans pin their own snapshot until the cursor closes; separate SELECT
statements in a read-only SQL transaction can observe different snapshots.
ULMDB allows readers while a writer is active and serializes writers. SQLite's
catalog-file locks still apply; WAL mode is useful for concurrent connections.

One ULMDB virtual table can be attached per SQLite connection. Use separate
connections for separate environments. A query may join that table with ordinary
SQLite tables, but a transaction that writes both stores is **not crash atomic**:
ULMDB and SQLite do not share a commit log or a recovery coordinator. Back up the
ULMDB directory separately; `.backup` only copies the SQLite catalog. Creating
or dropping the virtual table attaches or detaches storage; **DROP TABLE keeps
the ULMDB records**, and environment creation may persist after SQL rollback.

The cooperative module's versioned ULMDB named database is `sql_kv_v1`. Keys are signed-order-preserving
eight-byte big-endian integers (sign bit flipped), and values are raw blobs,
matching the PostgreSQL adapter's default layout. Do not write that database
through an unrelated client with a different encoding. Concurrent writes from
different SQL engines do not share SQL transaction isolation.
The owned module uses the same row encoding in its separate snapshot-log format;
this is an encoding convention, not file-format interoperability.

The module is direct-only: views and triggers cannot invoke it. Loading an
extension and choosing filesystem paths are trusted application operations.
General schemas, SQL indexes, triggers on virtual tables, full SQLite transaction
snapshot equivalence, VFS/page replacement, and cross-store recovery are outside
this initial adapter's scope.

Run the focused real-SQL tests with:

```sh
make -C storage-engines/sqlite test
make -C storage-engines/sqlite test-owned
make -C storage-engines/sqlite test-owned-capacity
```

The owned tests additionally exercise shared-process snapshots, competing
writers using separate SQLite catalogs, another process's refused open,
release of the owner lock, and reopening committed data.
The capacity test targets the selected versioned profile: a staged transaction
fits the SQL staging budget but exceeds the native value pool at COMMIT. It
checks that SQL receives `SQLITE_FULL`, earlier data remains intact, and the next
small transaction commits and reopens correctly.
