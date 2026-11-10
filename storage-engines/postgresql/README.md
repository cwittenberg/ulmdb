# PostgreSQL ULMDB tables

Experimental FDW for keyed blobs. PostgreSQL superuser required for
**all setup and access**.

## Build

Requires Rust 1.94.0, a C toolchain and matching PostgreSQL headers/PGXS
(17 tested). From repository root:

```sh
cargo build --release --locked -p ulmdb-c --features sql-batch # or sql-hash
make -C storage-engines/postgresql ULMDB_SQL_BATCH=1
sudo make -C storage-engines/postgresql ULMDB_SQL_BATCH=1 install
sudo install -d -m 700 -o postgres -g postgres /var/lib/postgresql/ulmdb-demo
```

Both options default off. Hash builds bounded indexes (65,536 keys across
databases) on first lookup per read transaction, favoring repeated reads.
Legacy mode requires SQL-adapter-created bytewise databases; native-order validation needs `sql-batch`.

Use an empty external directory. Override `PG_CONFIG`/`ULMDB_LIBDIR` on both
make commands; keep `libulmdb_c.so` at its runtime path.

## Use

```sql
CREATE EXTENSION ulmdb_fdw;
CREATE SERVER ulmdb_local FOREIGN DATA WRAPPER ulmdb_fdw
  OPTIONS (path '/var/lib/postgresql/ulmdb-demo');
CREATE FOREIGN TABLE fast_kv (id bigint NOT NULL, value bytea NOT NULL)
  SERVER ulmdb_local OPTIONS (database 'sql_kv_v1');

BEGIN;
INSERT INTO fast_kv VALUES (1, decode('68656c6c6f', 'hex'));
SELECT id, value FROM fast_kv WHERE id = 1;
COMMIT;
```

## Support and limits

| Area | Behavior |
| --- | --- |
| Schema | Exactly `id bigint NOT NULL, value bytea NOT NULL`; unique keys, empty blobs allowed. |
| Reads | Equality, IN/ANY (≤65,536 candidates), scans; PostgreSQL handles other filters/sorting. |
| Writes | INSERT, value UPDATE, DELETE, RETURNING; `id` cannot change. |
| Transactions | READ COMMITTED only; snapshot fixed at first read until transaction end. Read→write conflicts: `40001`, retry transaction. One environment/transaction. |
| Unsupported | Savepoint/subtransaction writes, write 2PC, COPY FROM, MERGE, ON CONFLICT, row locks, indexes, TRUNCATE, partition routing, ANALYZE. |
| Commit | Synchronous, experimental durability. Separate from PostgreSQL commit: crashes, cancellation or later callback errors can commit ULMDB while PostgreSQL aborts. |
| Backup | PostgreSQL WAL, replication, pg_basebackup and PITR exclude external files; back them up separately. pg_dump requires `--include-foreign-data`. |
| Lifecycle | DROP retains data. Restore/rollback/file replacement require offline access. Share files only with SQLite `ulmdb`, using sequential access. |
| Capacity | 1 GiB initial map; 128 named databases. |

Tests and benchmarks: [shared runner and performance chart](../README.md).
