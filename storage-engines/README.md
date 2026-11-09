# SQL storage adapters

Optional keyed SQL tables backed by ULMDB. Core code and default storage stay unchanged.

| Database | Interface | Build and use |
| --- | --- | --- |
| SQLite | `ulmdb_owned` (one process) or `ulmdb` (cooperative) virtual tables | [SQLite quickstart](sqlite/README.md) |
| PostgreSQL | `ulmdb_fdw` foreign tables | [PostgreSQL quickstart](postgresql/README.md) |

## Native database vs ULMDB

![Native SQLite and PostgreSQL without ULMDB versus ULMDB-backed SQL tables](performance.svg)

**Gray = native tables without ULMDB. Teal = ULMDB-backed tables.** Longer bars are
faster; right-hand ratios show ULMDB/native. Bars are medians; whiskers show trial ranges.

| Measurement | Values | Rows | Point queries | Trials |
| --- | --- | ---: | ---: | ---: |
| SQLite 3.54.1, `ulmdb_owned` | 64 B, 1 KiB, 2 KiB | 10,000 | 100,000 | 6 |
| PostgreSQL 17.11, FDW; earlier run | 64 B | 10,000 | 10,000 | 3 |

- Same SQL and data per pair; warm cache, one client; 1,000 rows/commit; `IN` batches of 100.
- SQLite: WAL/FULL, EXCLUSIVE, 4 KiB pages, 64 MiB cache/checkpoint budget.
- PostgreSQL: fsync, synchronous_commit and full_page_writes enabled.
- SQLite 2 KiB writes improve substantially; small records and most PostgreSQL workloads remain slower.

## Test both adapters

```sh
storage-engines/run-docker.sh
```

Docker builds dependencies and runs SQL, ownership, commit-failure and interoperability checks.

**Experimental:** external ULMDB files need separate backups. Native SQL and
ULMDB writes do not share an atomic commit. See each quickstart's limits.
