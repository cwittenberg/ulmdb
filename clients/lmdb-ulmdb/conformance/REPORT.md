# LMDB functional conformance

Generated 2026-11-04T06:10:51Z. Profile: `lmdb-common-functional-v1`.

**PASS**: 24 recorded checks. Each behavioral differential scenario runs an unchanged executable against the original LMDB shared library and the ULMDB replacement. Return codes and logical traces must match byte for byte. The [JSON receipt](receipt.json) records exact source commits, source hashes, executable hashes, trace hashes, compiler and adapter artifact.

| Scenario | LMDB 0.9.31 | LMDB 1.0.2 |
| --- | --- | --- |
| Common public C symbols | 56/56 exported | 56/56 exported |
| LMDB 1.0 catalog predicate export | NOT RUN | PASS |
| Header constants and ABI layouts | PASS | PASS |
| Common API | PASS | PASS |
| Ordering and duplicate flags | PASS | PASS |
| Reserve buffer lifetimes | PASS | PASS |
| Cursor deletion positioning | PASS | PASS |
| Database clear and drop | PASS | PASS |
| Environment and transaction contract | PASS | PASS |
| Cursor input and write contract | PASS | PASS |
| Randomized KV and duplicates (seed 1, 4096 operations) | PASS | PASS |
| Randomized KV and duplicates (seed 42, 4096 operations) | PASS | PASS |
| Randomized KV and duplicates (seed 314159265, 4096 operations) | PASS | PASS |
| Deployment and automatic migration (15 groups) | PASS | PASS |

The compatibility profile covers the tested logical transaction, key/value, duplicate, cursor, ordering and migration behavior of the common API. It accepts automatic conversion to native ULMDB storage with the original LMDB data retained. Keys and duplicate values are limited to 511 bytes, and ordinary values to 64 MiB.

Raw LMDB file/page identity, fixed-address mappings, persisted mapped pointers, identical physical statistics or capacity thresholds, and LMDB 1.0-only extensions other than `mdb_cursor_is_db` are outside this profile. The [remaining parity gaps](../PARITY_GAPS.md) list missing APIs and behavioral differences. The test results establish empirical conformance for the recorded cases; they do not prove every possible execution, performance equivalence, or production durability. Modern-only symbols and their availability are enumerated in the receipt.

Reproduce from the repository root:

```sh
make -C clients/lmdb-ulmdb
make -C clients/lmdb-ulmdb parity
```
