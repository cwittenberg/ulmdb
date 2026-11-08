# LMDB → ULMDB adapter

ULMDB with LMDB's library names, headers and `mdb_*` calls.

## Build

```sh
# Repository root; requires Cargo, Make and a C compiler
make -C clients/lmdb-ulmdb package ARGS='--fetch-legacy'

# Later builds: reuse cached sources
make -C clients/lmdb-ulmdb
```

- Output: `target/lmdb-ulmdb/{bin,lib,include}`; system libraries stay untouched.
- `--fetch-legacy`: downloads and verifies pinned LMDB 0.9.31 sources; requires `curl`.
- Release/custom output: `ARGS='--profile release --output /path/to/package'`.
- Linux and macOS 10.12+; macOS requires Apple's command-line tools and SDK.
- [Windows build and use](WINDOWS.md)

## Use

```sh
# Run a dynamically linked LMDB application; repeat for each launch
target/lmdb-ulmdb/bin/convert-lmdb-to-ulmdb ./application

# Select LMDB 1.0 behavior
target/lmdb-ulmdb/bin/convert-lmdb-to-ulmdb --abi 1.0.2 ./application
```

- `--abi`: `0.9.31`, `0.9.36` or `1.0.2`; a persisted `LMDB-ABI` marker takes priority.
- Native environments without that marker need the requested ABI on every open.
- Static/bundled LMDB: rebuild or relink against the adapter.
- macOS: protected/hardened applications may reject loader overrides.
- `MDB_VL32` is unsupported.

```sh
# Build C application source against the adapter
cc -I target/lmdb-ulmdb/include app.c \
  -L target/lmdb-ulmdb/lib -llmdb -o app
target/lmdb-ulmdb/bin/convert-lmdb-to-ulmdb ./app

# Optional: migrate ahead of time to an absent destination
target/lmdb-ulmdb/bin/ulmdb-migrate /path/to/lmdb /path/to/new-ulmdb
```

- First open automatically imports existing LMDB data into `PATH/.ulmdb` (directory) or `PATH.ulmdb` (single file).
- Later opens reuse that copy; original LMDB files are preserved.
- Explicit migration: point the application at the new destination.
- Original and imported databases are separate; writes are not mirrored. Stop writing the original after switching.
- Custom comparators: register them again in the application.
- [C API](../c/include/lmdb.h) · [IP extensions](../c/include/ulmdb.h)

## Test

```sh
make -C clients/lmdb-ulmdb test
make -C clients/lmdb-ulmdb parity
```

- [Compatibility results](conformance/REPORT.md) · [Unsupported features and limits](PARITY_GAPS.md)
