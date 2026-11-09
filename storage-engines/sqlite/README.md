# SQLite quick reference

Requires Linux, Rust 1.94, make, C compiler, SQLite 3.38+ headers/CLI.

| Module | Extension | Storage |
| --- | --- | --- |
| `ulmdb` | `ulmdb_sqlite.so` | Cooperative clients; existing ULMDB format/C library |
| `ulmdb_owned` | `ulmdb_sqlite_owned.so` | One owning process, shared connections; separate snapshot-log format |

## Build

From repository root:

```sh
# Cooperative
cargo build --release --locked -p ulmdb-c
make -C storage-engines/sqlite

# Owned (versioned profile)
make -C storage-engines/sqlite owned
```

Cooperative deployment needs `libulmdb_c.so`; customize its location with
`ULMDB_LIBDIR=/absolute/library/directory`. Owned prebuilt linking skips Cargo:

```sh
make -C storage-engines/sqlite owned \
  ULMDB_NATIVE_LIB=/absolute/libulmdb_sqlite_native.a
```

[Direct Rust commands](native/README.md).

## Use

`sqlite3 /tmp/catalog.sqlite`; choose one:

```sql
.load ./storage-engines/sqlite/ulmdb_sqlite
CREATE VIRTUAL TABLE cache USING ulmdb('/tmp/cooperative-cache');
```

```sql
.load ./storage-engines/sqlite/ulmdb_sqlite_owned
CREATE VIRTUAL TABLE cache USING ulmdb_owned('/tmp/owned-cache');
```

Use separate directories with existing parents. Owned files exclude cooperative
and PostgreSQL adapters.

```sql
BEGIN;
INSERT INTO cache(id,value) VALUES(42, CAST('hello' AS BLOB));
COMMIT;
SELECT id,value FROM cache WHERE id IN(42,7,100) ORDER BY id;
UPDATE cache SET value=CAST('updated' AS BLOB) WHERE id=42;
DELETE FROM cache WHERE id=42;
```

## Limits

| Area | Limit/behavior |
| --- | --- |
| Schema | Fixed `id INTEGER PRIMARY KEY, value BLOB NOT NULL`; explicit signed 64-bit keys; `rowid=id`. Empty blobs allowed. Change keys through `id`. |
| Owned staging | 65,536 changed keys; 64 MiB including undo/metadata; 1,024 nested savepoints. |
| Owned storage | 65,536 live slots; 64 MiB index including 32 MiB value pool; 64 MiB/log bank; 16 MiB/blob. Retained snapshots consume capacity; COMMIT can refuse. |
| IN | 65,536 candidates; 64 MiB blob data. |
| Transactions | One synchronous writer; rollback/savepoints supported; reads see own writes. |
| Reads | Statement/IN snapshots, including inside BEGIN. Only INTEGER lower bounds seek; SQLite filters others. |
| Ownership | Owned excludes other processes. Active handles are thread-bound. Close all connections before moving/restoring files. |
| Commit | Publishes in `xSync`; later SQLite failures can abort SQL after publication. Rollback cannot undo publication; mixed-store writes lack atomicity. |
| Backup/drop | `.backup` copies catalog only. Back up external data separately with owned stores closed. DROP retains files; creation can survive rollback. |
| SQL | One table/module/connection; fixed schema, no secondary indexes, views or triggers (direct-only). |

Owned equality/IN follows numeric affinity. REAL(-2^63) follows ordinary
comparison; SQLite 3.40.1/3.45.1 indexed-rowid seeks reject that boundary.

[Checks, status, measurements](../README.md).
