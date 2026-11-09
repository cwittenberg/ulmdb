# Optional SQLite and PostgreSQL storage adapters

Optional extensions expose ULMDB-backed tables through ordinary SQL. They build
separately from the Cargo workspace and require **no ULMDB core changes**.
SQLite offers a cooperative C-library module and an embedded owner module;
PostgreSQL uses the existing C library. Both databases keep their default storage.

| Database | Source | Build/use guide | Integration |
| --- | --- | --- | --- |
| SQLite | [sqlite/ulmdb_sqlite.c](sqlite/ulmdb_sqlite.c), [native bridge](sqlite/native/) | [SQLite guide](sqlite/README.md) | Loadable virtual-table modules `ulmdb` and `ulmdb_owned` |
| PostgreSQL | [postgresql/ulmdb_fdw.c](postgresql/ulmdb_fdw.c) | [PostgreSQL guide](postgresql/README.md) | PGXS extension `ulmdb_fdw`, installed with `CREATE EXTENSION` |

The initial schema is an explicit signed 64-bit `id` and a non-null binary
`value`. Inserts, reads, updates and deletes use ULMDB storage; equality lookups
use its key index. Arbitrary existing SQL schemas are not converted. PostgreSQL
uses a foreign data wrapper rather than `CREATE TABLE ... USING ulmdb`.
SQLite's virtual-table boundary replaces the table's B-tree path; a VFS would
retain SQLite's B-tree and pager and add ULMDB beneath them. PostgreSQL's standard
PGXS and `CREATE EXTENSION` packaging is similar to pgvector's, while its FDW
provides external table storage. A native PostgreSQL Table AM would additionally
need tuple identities, PostgreSQL visibility, index integration and coordinated
WAL recovery before it could safely replace general table storage.

The optional `ulmdb_owned` module gives one process exclusive ownership of an
existing native snapshot-log engine. SQLite connections in that process share
its owner and obtain inexpensive immutable snapshots. This avoids repeating
cooperative file admission for each query. It uses a separate disk format;
other processes cannot open that environment while its owner remains live.
The cooperative `ulmdb` module remains available for interoperability with
the C client and PostgreSQL adapter.

PostgreSQL retains one external read snapshot until its SQL transaction ends;
use an explicit transaction to group reads. SQLite statements acquire fresh
external snapshots; `IN (...)` resolves its key set under one snapshot. Direct
key lookups, native ordered cursors and bounded write sets serve the SQL paths.
The C client adds an immutable base-checkpoint accessor for safe PostgreSQL
read-to-write promotion and combines reader-record writes into one buffer.
ULMDB's storage, publication, admission checks and locking remain unchanged.

## Build and use

For the embedded SQLite module on Linux, use Rust, a C compiler and SQLite
development headers. From the repository root:

```sh
make -C storage-engines/sqlite owned
sqlite3 /tmp/catalog.sqlite
```

```sql
.load ./storage-engines/sqlite/ulmdb_sqlite_owned
CREATE VIRTUAL TABLE cache USING ulmdb_owned('/tmp/owned-cache');
INSERT INTO cache VALUES(42, CAST('hello' AS BLOB));
SELECT id,value FROM cache WHERE id=42;
```

Use a new environment directory; its parent must exist. For the cooperative
module instead, build the C library and load its extension:

```sh
cargo build --release --locked -p ulmdb-c
make -C storage-engines/sqlite
mkdir -p /tmp/ulmdb-sql
sqlite3 /tmp/catalog.sqlite
```

```sql
.load ./storage-engines/sqlite/ulmdb_sqlite
CREATE VIRTUAL TABLE cache USING ulmdb('/tmp/ulmdb-sql');
INSERT INTO cache(id,value) VALUES(42, CAST('hello' AS BLOB));
SELECT id,value FROM cache WHERE id=42;
```

For PostgreSQL, install the development headers matching the server and make
sure `pg_config` identifies that installation. Deploy the ULMDB library at a
stable path readable by the server, then build against that path:

```sh
make -C storage-engines/postgresql ULMDB_LIBDIR=/absolute/ulmdb/library
make -C storage-engines/postgresql ULMDB_LIBDIR=/absolute/ulmdb/library install
# Create /var/lib/postgresql/ulmdb-cache owned by the PostgreSQL account.
```

As a PostgreSQL superuser:

```sql
CREATE EXTENSION ulmdb_fdw;
CREATE SERVER ulmdb_local FOREIGN DATA WRAPPER ulmdb_fdw
  OPTIONS(path '/var/lib/postgresql/ulmdb-cache');
CREATE FOREIGN TABLE cache(id bigint NOT NULL, value bytea NOT NULL)
  SERVER ulmdb_local OPTIONS(database 'sql_kv_v1');
INSERT INTO cache VALUES(42, decode('68656c6c6f','hex'));
SELECT id,value FROM cache WHERE id=42;
```

These are experimental keyed-table interfaces. Read each adapter's transaction
and operation limits before use. SQL-native data and ULMDB do not share
crash-atomic commit, PostgreSQL physical replication or PITR. The PostgreSQL
adapter currently requires superuser access and READ COMMITTED isolation.
Back up the external ULMDB files as well as the SQL definitions.

## SQL performance

![ULMDB-backed SQLite and PostgreSQL throughput relative to native tables](performance.svg)

SQLite measurements compare `ulmdb_owned` with native SQLite **3.54.1** on
Linux/ext4: 10,000 keys, 64-byte/1 KiB/2 KiB values, 100,000 warm random point
requests, `IN` batches of 100, and 1,000 rows per synchronous write transaction.
Each profile uses six complete trials, with three native-first and three
ULMDB-first runs. Bars divide median ULMDB throughput by median native
throughput; dots show each paired trial. Both variants use identical SQL, data,
client calls and byte/count/SHA256 validation. Timings include execution,
returned bytes, client work and commits.

Both SQLite variants use WAL/FULL, EXCLUSIVE locking, 4 KiB pages and a 64 MiB
page-cache setting. The WAL checkpoint threshold is explicitly 16,384 pages
(64 MiB), matching the owned engine's 64 MiB log-bank budget rather than
comparing it with SQLite's smaller default threshold. The native owner has a
64 MiB authoritative index budget and two log banks. Capacities, snapshots and
recovery guarantees still differ; see the [SQLite guide](sqlite/README.md).

With **2 KiB values**, the owned module reached **2.04× load** and **2.89× update**
throughput. Its batched reads and scans reached **1.06×**, while point reads were
close at **1.02×**; that small point margin overlaps trial variation. All five
medians exceeded native in this profile. At 1 KiB, loads and updates improved
but point reads and scans remained slightly slower. At 64 bytes, four of five
workloads were slower. The gain depends on record size and this ownership model.

The PostgreSQL 17.11 panel retains the earlier three-trial reference: 10,000
64-byte rows and 10,000 point requests, with fsync, synchronous_commit and
full_page_writes enabled. Its point reads reached **1.14×** native throughput;
the remaining workloads were slower. It was measured separately and was not
rerun as part of the SQLite optimization.

These are warm, single-client keyed workloads. They do not establish a general
SQL speedup, identical isolation, cold-cache performance, sustained checkpoint
throughput or equivalent crash recovery. Small-record losses remain visible.
The larger 4 KiB/8 KiB diagnostic configurations exceeded the owned module's
fixed capacity and are not represented as successful throughput results.

Benchmark scripts, raw observations and internal design notes remain local and
ignored. Only this chart and README documentation are included with the source.

## Verify

Run the build, focused adapter checks and cross-adapter format check inside an
isolated container:

```sh
storage-engines/run-docker.sh
```

Docker is the only host dependency for this route. Rust and the PostgreSQL,
SQLite and Python development/test dependencies stay inside the image. Network
access is used to build the image; tests run with networking
disabled, PostgreSQL listening only on a Unix socket, and an unprivileged user.
Generated database files use a new directory on the checkout's filesystem. The
command prints its retained data directory. Checks cover SQL transactions,
snapshot freshness, concurrent update conflicts, process/thread ownership,
bounded batches, real owned-store commit-capacity refusal, cooperative commit
fault injection and sequential SQLite/PostgreSQL interoperability. The additive C
checkpoint accessor is also tested against native branch publication that
changes data while reusing the same transaction sequence number.
