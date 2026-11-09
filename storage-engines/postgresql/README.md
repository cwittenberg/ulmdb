# PostgreSQL ULMDB tables

This optional, experimental foreign data wrapper stores keyed tables in ULMDB
and executes SQL through PostgreSQL's existing parser, planner and executor.
It links the existing ULMDB C client; it changes no ULMDB core code and requires
no PostgreSQL fork. It is **an FDW, not a native PostgreSQL table access method**.
Install it only when you want ULMDB-backed tables. Ordinary tables keep using
PostgreSQL's default storage.

## Build and use

Requires a C compiler, make, the development headers and PGXS matching your
PostgreSQL server, and Rust 1.94.0 for the existing ULMDB library. PostgreSQL 17
is the version exercised by the supplied Docker runner. Build from repository
root:

```sh
cargo build --release --locked -p ulmdb-c
make -C storage-engines/postgresql
sudo make -C storage-engines/postgresql install
```

`PG_CONFIG=/path/to/pg_config` selects the target installation.
`ULMDB_LIBDIR=/absolute/library/directory` selects `libulmdb_c.so`; both make
commands must use the same setting. The module embeds that directory as its
runtime search path. Keep the library there or install it in your system's
loader path. Match the library's operating system/architecture/libc to the
PostgreSQL server.

Prepare an **empty, dedicated** ULMDB directory owned by the PostgreSQL service
account. It must exist before use; do not use PostgreSQL's data directory:

```sh
sudo install -d -m 700 -o postgres -g postgres /var/lib/postgresql/ulmdb-demo
```

Run as a PostgreSQL superuser:

```sql
CREATE EXTENSION ulmdb_fdw;
CREATE SERVER ulmdb_local FOREIGN DATA WRAPPER ulmdb_fdw
  OPTIONS (path '/var/lib/postgresql/ulmdb-demo');
CREATE FOREIGN TABLE fast_kv (
  id bigint NOT NULL,
  value bytea NOT NULL
) SERVER ulmdb_local OPTIONS (database 'sql_kv_v1');

BEGIN;
INSERT INTO fast_kv VALUES (1, decode('68656c6c6f', 'hex'));
UPDATE fast_kv SET value = decode('776f726c64', 'hex') WHERE id = 1;
COMMIT;

SELECT id, convert_from(value, 'UTF8') FROM fast_kv WHERE id = 1;
EXPLAIN SELECT value FROM fast_kv WHERE id = 1;
-- ULMDB access: primary-key lookup

BEGIN;
DELETE FROM fast_kv WHERE id = 1;
ROLLBACK;
```

The initial schema is exactly `id bigint NOT NULL, value bytea NOT NULL`, in that
order and with those names. `id` is the unique signed 64-bit key; PostgreSQL
does not permit a `PRIMARY KEY` constraint on a foreign table, so the adapter
enforces uniqueness through ULMDB. Empty bytea is valid; null values are rejected.
Encode application records in `value` when a keyed blob model fits your workload.
This release does not provide arbitrary SQL column schemas or secondary indexes.

## Execution and transaction boundaries

- SELECT supports complete ordered key scans and primary-key equality pushdown
  for builtin integer equality with constants or query parameters. `id IN (...)`
  and `id = ANY(integer_array)` also push down as batches of direct lookups,
  including prepared scalar parameters or an array parameter. Each batch is
  evaluated once per scan/rescan, sorts and deduplicates signed keys, and skips
  null candidates and missing keys. It accepts at most 65,536 array elements;
  larger pushed-down batches fail explicitly. PostgreSQL rechecks the original
  predicate. EXPLAIN identifies this path as `ULMDB access: primary-key batch`.
  Batch arrays may use `smallint`, `integer` or `bigint` elements. Other
  predicates execute in PostgreSQL after the scan. `ORDER BY` is evaluated by
  PostgreSQL; the adapter does not advertise sorted path keys.
- INSERT, value UPDATE, DELETE and their RETURNING clauses use one ULMDB write
  transaction for the PostgreSQL transaction. Reads after a write see that
  transaction's changes. COMMIT publishes at PostgreSQL's pre-commit callback;
  ROLLBACK and top-level errors abort. Mutating the primary key is rejected;
  use DELETE and INSERT inside a transaction.
- The first read in a PostgreSQL transaction captures one real ULMDB snapshot;
  subsequent scans and queries reuse it until COMMIT or ROLLBACK. Concurrent
  external commits become visible in the **next PostgreSQL transaction**.
  This external snapshot stays fixed even under READ COMMITTED, rather than
  adopting PostgreSQL heap's per-statement MVCC snapshot. Only READ COMMITTED
  is accepted; stronger isolation is rejected. No reader survives transaction
  end. Use an explicit transaction to amortize snapshot setup across lookups.
- When a transaction reads before its first write, the adapter acquires the
  ULMDB writer and compares its immutable base checkpoint with the retained
  read snapshot's checkpoint, including changes that reuse a sequence number.
  A concurrent commit causes SQLSTATE `40001` (serialization failure); retry
  the whole transaction. On success later scans read the writer, including its
  own changes. Existing scans retain their earlier snapshot until completion.
  Native rollback, restore and file replacement are maintenance operations;
  run them offline after all SQL backends close the environment.
- Writes in SAVEPOINT/subtransactions/PL/pgSQL exception blocks are rejected
  before mutation. Two-phase `PREPARE TRANSACTION` is rejected when ULMDB was
  enlisted for writing. ON CONFLICT, MERGE, COPY FROM, TRUNCATE, partition
  routing, row locking, foreign-table ANALYZE and secondary indexes are not
  implemented. Do not use row-locking clauses for concurrency control.
- A transaction may access one environment. Multiple foreign tables in that
  environment share the writer, and may select different named databases.
  The initial configured map size is 1 GiB with at most 128 named databases.
- ULMDB commits are synchronous (no relaxed durability flags). ULMDB still has
  the experimental durability status documented in the root README.
- PostgreSQL WAL, streaming/logical replication, `pg_basebackup`, crash recovery
  and PITR do **not** include the external ULMDB files. `pg_dump` does not export
  foreign rows unless explicitly requested with `--include-foreign-data`.
  Back up the external data separately. DROP FOREIGN TABLE/EXTENSION removes
  catalog entries; it does not delete ULMDB records or files.
- ULMDB publication and PostgreSQL's heap/catalog commit are separate events.
  A crash, later pre-commit callback error or cancellation after ULMDB commits can leave it
  committed while the PostgreSQL transaction aborts. There is no crash-atomic
  commit across ULMDB and PostgreSQL tables or other FDWs. Use isolated ULMDB
  transactions for experiments requiring this adapter's documented boundary.

All configuration **and access** currently require a superuser because the FDW
opens server-local paths. Server/table grants alone do not enable ordinary
users. No network service or sidecar is needed. Prefer a dedicated test cluster.

Keys use eight-byte, big-endian signed-order encoding (sign bit flipped), with
raw blob values. `database 'sql_kv_v1'` matches the SQLite adapter. Close one
engine before opening the same environment with the other; the interoperability
test exercises sequential access, not concurrent mixed-engine transactions.

## Verify and benchmark

`python3 storage-engines/postgresql/test.py` uses libpq's `PGHOST`, `PGUSER` and
`PGDATABASE` to connect to an isolated test server. Its process and the server
must share `/tmp` (or `ULMDB_PG_TEST_ROOT`). It installs the extension in that
test database, creates temporary fixtures, and checks real SQL transactions,
signed keys, equality plans, query parameters and row mutations. See the
[shared runner and performance chart](../README.md) for
containerized setup and measurements against PostgreSQL heap tables.

PostgreSQL's extension APIs are documented in the official
[FDW callbacks](https://www.postgresql.org/docs/17/fdw-callbacks.html) reference.
A full native table engine also needs PostgreSQL tuple identities, MVCC,
WAL/recovery, index maintenance and vacuum integration.
