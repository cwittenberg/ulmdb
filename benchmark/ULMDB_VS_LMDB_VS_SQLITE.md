# ULMDB Resident vs LMDB vs SQLite — 5 November 2026

These are historical **Resident** results from three recorded runs per engine.
They do not measure the default Legacy backend or the later snapshot-log backend.
No benchmark was rerun to restore this report.

Warm-read throughput was **1.67× LMDB and 6.10× SQLite** in this workload.
Load throughput remained below LMDB, and Resident incurred substantially higher
setup, reopen and storage costs.

## Throughput

Each run loaded 1,000,000 keys, performed 10,000,000 warm reads and applied
1,000,000 updates. Writes used 1,000 records per durable transaction. Values below
are geometric means of the three per-run rates, in operations per second; higher
is better. Ratios use the unrounded rates.

| Operation | ULMDB Resident/s | LMDB/s | SQLite/s | ULMDB / LMDB | ULMDB / SQLite |
| --- | ---: | ---: | ---: | ---: | ---: |
| Load | 1,396,940 | 1,539,753 | 817,306 | 0.91× | 1.71× |
| Read | 2,776,989 | 1,660,288 | 455,490 | 1.67× | 6.10× |
| Update | 145,257 | 128,143 | 58,247 | 1.13× | 2.49× |

All nine recorded profiles completed and passed the initial/final value and miss
checks, exact row count, timed-read hit/checksum check and reopened-value check.
The profiles use identical dataset, query and update hashes. LMDB and SQLite
controls were shared across the historical campaign's ULMDB arms; they are not
additional independent control runs for each arm.

## Setup, reopen and storage

These costs are separate from throughput. Geometric means of three runs are shown;
lower is better. File sizes include the environment inventory after close/reopen.

| Engine | Setup, ms | Close/reopen, ms | Apparent MiB | Allocated MiB |
| --- | ---: | ---: | ---: | ---: |
| ULMDB Resident | 408.683 | 7,088.012 | 256.000 | 256.001 |
| LMDB | 1.225 | 3.892 | 33.047 | 33.043 |
| SQLite | 2.708 | 9.380 | 24.145 | 24.145 |

Close/reopen kept OS caches. Resident performed full WAL replay and graph audit;
the full reopened-value oracle ran outside the close/reopen timer. SQLite's
close/reopen includes close/checkpoint work. Allocated storage includes reserved
capacity. Resident WAL retirement was disabled in this profile. These aggregate
results provide no per-operation p95 or p99 measurements.

## Recorded method and configuration

- One process and one thread, CPU affinity 0; AMD EPYC 7702P, x86-64 Linux
  `6.8.0-147-generic`. Disk storage was reported as the ext2/ext3 filesystem family.
- Eight-byte keys and eight-byte values, seed 73. Initial load was sorted; updates
  used shuffled keys. Warm reads used one bound snapshot, with 5,000,847 hits out
  of 10,000,000 queries. Snapshot binding and validation ran outside the timers.
- Write timers included begin, mutation and commit; automatic Resident collection
  ran inside begin-write. Read timers included the loop and checksum work.
- ULMDB used `ulmdb::resident::Environment CompactV2 Borrowed`, 8,192 KV pages,
  zero Tiny pages, a 256 MiB WAL, a 2,048-page collection threshold, and 64 MiB
  configured limits each for reserved pool bytes and metadata. No Legacy fallback
  or relaxed sync was used.
- LMDB 1.0.2 used ordinary bytewise keys, normal durable flags `0`, a 1 GiB map and
  a prepared read handle.
- SQLite 3.45.1 used native prepared statements, `BLOB PRIMARY KEY WITHOUT ROWID`,
  WAL, `synchronous=FULL`, 4 KiB pages, a 2,000 KiB cache, disabled mmap, and
  `wal_autocheckpoint=1000`. Engine-private capacities and cache settings differ.

## Data and provenance

The source is [results/millions.json](results/millions.json), captured on
5 November 2026 and published with timestamp `2026-11-05T13:43:35.425992+00:00`.
Its SHA-256 is
`48fb689678689fec1c71acdac88fa354c539043957c83142d8fbb91a24b5f7ad`.
This report recomputes each rate as `operations * 1e9 / elapsed_ns` and then takes
the geometric mean; it does not substitute rates from another workload.

The JSON records the executable SHA-256
`8df2ee6b08cc57d09841e72b5de9554d5303d8a662e218481e183c5ca8a1401c`,
compiler/build information, native library identity, per-run timers, exact
settings and the captured source hash map. Selected recorded source identities:

| Recorded source path | SHA-256 |
| --- | --- |
| `benchmark/millions/src/workload.rs` | `0e9ba5afadd71916ebf67bad6ac1fb04218c50184211469aee245c4c9abcbae6` |
| `benchmark/millions/src/report.rs` | `24665031405b5eff6df643248d9b5e2d5e06a8a2d8b4018eaa6e09174813be25` |
| `benchmark/millions/src/ulmdb_resident.rs` | `beb43748356c838ecc4b1aa10ce69849fb0c3da0b2b408f85ac1fb83cc1ed8c1` |
| `benchmark/millions/src/lmdb_backend.rs` | `0828252d2cc8afdf098f9e4cb2d9038a75393ba5d1d2e20d199c2a2e0d02f274` |
| `benchmark/millions/src/sqlite.rs` | `a7253c513c1414d928da1bf7a9746e7fd5e13ae303d3872514736cf13788f4ef` |

These identities describe the recorded build, not necessarily today's source.
The JSON marks the experimental profile as unqualified. The measurements do not
establish universal performance, production durability or default-backend parity.
