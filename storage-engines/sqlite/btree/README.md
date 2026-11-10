# SQLite B-tree prototype

Experimental SQLite 3.54.1 build: ULMDB stores schema and rows; SQLite keeps
its SQL compiler and VM. No speedup is qualified.

| Backend | Build target | Files to preserve |
| --- | --- | --- |
| Native snapshot-log | `native` | Entire database directory; one owning process |
| C SDK | Default | Marker file **and** adjacent `.ulmdb` directory |

From repository root:

```sh
SQLITE_SRC=/path/to/sqlite-amalgamation-3540100
make -C storage-engines/sqlite/btree native SQLITE_SRC="$SQLITE_SRC"
target/sqlite-btree/sqlite3-ulmdb-native rows.native \
  'CREATE TABLE kv(id INTEGER PRIMARY KEY, value BLOB);'
make -C storage-engines/sqlite/btree test-native SQLITE_SRC="$SQLITE_SRC"

# C SDK alternative
cargo build --release --locked -p ulmdb-c --features sql-hash
make -C storage-engines/sqlite/btree SQLITE_SRC="$SQLITE_SRC"
target/sqlite-btree/sqlite3-ulmdb rows.db \
  'CREATE TABLE kv(id INTEGER PRIMARY KEY, value BLOB);'
make -C storage-engines/sqlite/btree test SQLITE_SRC="$SQLITE_SRC"
```

- Unsupported: persistent indexes, WITHOUT ROWID, ATTACH, backup, VACUUM and page APIs.
- FULL durable commits; active transactions stay on their owning thread.
- Native limits: 65,536 staged keys; 64 MiB including undo; 16 MiB/record; 64 MiB each for index and log budgets.
- Separate formats; ordinary SQLite files and other adapters are incompatible.

`generate.py` verifies the pinned amalgamation and port changes. The original
[LumoSQL backend and MIT license](lumosql/) are preserved with source hashes.
