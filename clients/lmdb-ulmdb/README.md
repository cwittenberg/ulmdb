# LMDB → ULMDB adapter

This package gives the ULMDB C interface LMDB's library names. Existing programs
keep their `lmdb.h` includes and `mdb_*` calls. Dynamically linked applications can select ULMDB through the platform loader
without recompiling, within the loader boundaries described below.
The package also migrates real LMDB snapshots into native ULMDB storage.

The adapter stores records in ULMDB. The reference LMDB implementation is linked
only into a separate, read-only migration exporter and the test reference library.
It is never linked into the ULMDB engine or replacement `liblmdb`.

The [conformance report](conformance/REPORT.md) records reproducible differential
results against pinned LMDB 0.9.31 and 1.0.2. The
[remaining parity gaps](PARITY_GAPS.md) list missing APIs and behavioral/storage
differences separately from the passing compatibility profile.

## Build

From the repository root:

```sh
make -C clients/lmdb-ulmdb
```

The default package is `target/lmdb-ulmdb`:

```text
lib/liblmdb.so.0           ULMDB implementation, ELF SONAME liblmdb.so.0
lib/liblmdb.so             linker alias
lib/liblmdb.so.1           alternate loader alias
lib/liblmdb.a              static ULMDB adapter
lib/libulmdb_c.so          native-client alias
include/lmdb.h            adapter C API
include/lmdb-0.9/lmdb.h   unchanged pinned LMDB 0.9.31 header
include/lmdb-1.0/lmdb.h   unchanged pinned LMDB 1.0.2 header
lib/pkgconfig/            explicit lmdb-ulmdb-0.9 and lmdb-ulmdb-1.0 profiles
include/ulmdb.h            native IPv4/IPv6 additions
bin/lmdb-export           isolated real-LMDB snapshot reader
bin/lmdb-export-0.9        isolated reader for LMDB 0.9 data files
bin/ulmdb-migrate          atomic snapshot import command
bin/convert-lmdb-to-ulmdb  launcher selecting the adapter and migration helper
share/licenses/           exporters' licenses, copyrights, source commits and notices
```

Build options are passed through `ARGS`, for example:

```sh
make -C clients/lmdb-ulmdb package ARGS='--profile release --output /path/to/package'
```

The native build supports Linux and macOS and requires Make, Cargo and a C compiler.
On macOS, install Apple command-line developer tools and an SDK. The library is
`liblmdb.0.dylib` with install name `@rpath/liblmdb.0.dylib`; `liblmdb.dylib`,
`liblmdb.1.dylib` and `libulmdb_c.dylib` are aliases. The `.so` aliases remain
for callers that explicitly name them. The operator uses `@loader_path/../lib`,
and the launcher selects `DYLD_LIBRARY_PATH` using its actual dyld executable
path. Existing application install names and macOS loader restrictions still
apply: protected or hardened applications can reject environment overrides.
This package does not modify their signing or loader policy.

New-directory operator loads and migrations on macOS use `renameatx_np` with
`RENAME_EXCL`, so an existing target is never replaced. Regular migration
metadata requires `F_FULLFSYNC`; publication fences the staged and parent
directories. Filesystems that reject those operations return an error; there is
no weaker successful publication fallback. macOS 10.12 or later is required.

The packager, launcher, exporters, importer and adapter tests are C programs; no Python builder is used.
Modern LMDB source defaults to the repository's
[vendored source](../../benchmark/support/lmdb/vendor), checked against the
[adapter exporter pins](modern-source.json) and
[canonical LMDB pins](../../benchmark/support/lmdb/source.toml). Set
`ULMDB_LMDB_SOURCE` or `--lmdb-source` to use another exact pinned checkout.
No verification, comparison-runner or ignored modern-source cache is required.

The LMDB 0.9.31 exporter has separate [legacy pins](legacy-source.json).
Its source defaults to `~/.cache/ulmdb-compat/lmdb-0.9.31`, or pass
`--legacy-source` explicitly. On a fresh checkout, fetch the exact legacy files
from the official HTTPS source with:

```sh
make -C clients/lmdb-ulmdb package ARGS='--fetch-legacy'
```

This explicit optional download uses `curl` and verifies every size and hash.
The ordinary build never downloads sources automatically.
The default build does not install or overwrite system libraries.


### Windows tools package

From the repository root, with the Rust Windows GNU target and MinGW tools
installed:

```sh
cargo build --locked --release -p ulmdb-c --target x86_64-pc-windows-gnu
python3 clients/lmdb-ulmdb/build_windows_tools.py \
  --modern-source benchmark/support/lmdb/vendor \
  --legacy-source "$HOME/.cache/ulmdb-compat/lmdb-0.9.31" \
  --dll target/x86_64-pc-windows-gnu/release/ulmdb_c.dll \
  --import-library target/x86_64-pc-windows-gnu/release/libulmdb_c.dll.a \
  --output target/lmdb-ulmdb-windows
```

This Python builder accepts native MinGW or a cross-toolchain. Its default
compiler is `x86_64-w64-mingw32-gcc` and its default inspector is
`x86_64-w64-mingw32-objdump`, resolved through `PATH`. Override them with `--cc`
and `--objdump`, each taking one executable path, including an absolute Windows
path when running natively. All source, DLL and output arguments above are
required; relative paths resolve from the invoking working directory. The output
directory must not exist. `--import-library` adds `ulmdb-tool.exe`; omit it only
when deliberately building migration/launcher tools without the operator.
The legacy source must already match its pins; this builder does not fetch it.

`bin/` contains the normal application launcher, importer, two independent pinned
LMDB exporters and operator, plus identical provider copies named `ulmdb_c.dll`,
`lmdb.dll`, `liblmdb.dll` and `libulmdb_c.dll`. `share/licenses/` retains each
exporter's LICENSE, COPYRIGHT, linked source/header files and exact source commit
and hashes. `BUILD.json` binds input, import-table and installed output hashes.
The exporters do not import the adapter DLL; the importer loads its explicitly
selected provider. Cross-compilation/link checks do not establish Windows runtime
behavior. The Darwin C tools also still require an Apple SDK/compiler and a real
macOS build; Linux builds and Rust cross-checks alone do not establish that result.

For an ordinary dynamically linked Windows executable:

```text
target/lmdb-ulmdb-windows/bin/convert-lmdb-to-ulmdb.exe --abi 1.0.2 C:/apps/application.exe
```

The launcher configures the package DLL directory and `PATH`, supplies adjacent
migration/provider paths unless explicitly configured, starts a normal child
process and forwards its exit status. It uses no debugger, injection, application
file edits or application job restriction. A competing known LMDB DLL beside the
application is rejected before launch; replace that application-local binding
explicitly. Statically embedded LMDB, private DLL names, absolute/private loader
bindings and application-specific loader changes remain outside this launcher
contract. Only the owned migration exporter has a bounded, kill-on-close job.

## When conversion is needed

A separate manual conversion step is optional:

- **New databases or existing native ULMDB databases:** no conversion is needed.
- **Existing LMDB databases:** the adapter automatically imports the data on its
  first open. It preserves the original files and uses the native copy on later
  opens. Your application can keep its existing database path.
- **Import ahead of time or choose another destination:** optionally run
  `ulmdb-migrate SOURCE DESTINATION`, then point the application at the new
  destination. See [explicit migration](#migrate-an-existing-lmdb-environment).

Existing LMDB data requires an import because ULMDB uses its own storage format.
The automatic import handles this without a separate conversion command.

## Run an existing application

For a program dynamically linked to `liblmdb.so.0`:

```sh
target/lmdb-ulmdb/bin/convert-lmdb-to-ulmdb ./existing-application
```

`convert-lmdb-to-ulmdb` is an optional launcher: its arguments are the application
to run and that application's arguments. It configures the library search path
and locates its adjacent migration helper for that run. Use the launcher each
time you start the application unless your loader or build configuration already
selects the adapter. Existing converted data is reused; launching again does not
repeat the import. The equivalent library override is:

```sh
LD_LIBRARY_PATH="$PWD/target/lmdb-ulmdb/lib${LD_LIBRARY_PATH:+:$LD_LIBRARY_PATH}" \
  ./existing-application
```

An existing program using the `liblmdb.so.1` name can use the supplied alias.
The library alias alone does not select transaction/catalog behavior. Choose the
runtime profile explicitly for a new environment:

```sh
target/lmdb-ulmdb/bin/convert-lmdb-to-ulmdb --abi 1.0.2 ./application
```

`--abi` accepts `0.9.31`, `0.9.36` and `1.0.2` and overrides inherited
`ULMDB_LMDB_ABI`. An existing `LMDB-ABI` marker retains priority on reopen.
Imported environments carry that marker; native environments without it require
the runtime request on each open.
For compilation, use `include/lmdb-0.9` or `include/lmdb-1.0`, or select
`lmdb-ulmdb-0.9` / `lmdb-ulmdb-1.0` through `pkg-config`. Each installed
profile header is byte-for-byte identical to its pinned upstream source.
The `ulmdb_abi` pkg-config variable reports the matching runtime selection.
The upstream `MDB_VL32` ABI remains unsupported and its renamed entry point
fails to link; do not mix that header option into these profiles.

New application builds can keep their source and ordinary LMDB link flag:

```sh
cc -I target/lmdb-ulmdb/include app.c \
  -L target/lmdb-ulmdb/lib -llmdb -o app
```

An application that statically embeds LMDB needs to be relinked or rebuilt against
this adapter. A language extension that bundles a private, hidden LMDB copy also
needs its build to use this library. Changing a library search path cannot replace
hidden code inside an existing binary. The application's LMDB API calls remain
the same in either case.

The [C adapter guide](../c/README.md) records supported APIs, flags, transaction
behavior and remaining limitations. Library-name compatibility alone does not
establish compatibility with every LMDB release or extension.

## Migrate an existing LMDB environment

With the packaged launcher, opening an existing LMDB path through the adapter
imports it automatically. A directory environment is imported into its `.ulmdb`
subdirectory; an `MDB_NOSUBDIR` file uses a sibling directory named
`ORIGINAL_PATH.ulmdb`. Subsequent adapter opens use that native directory. The
original LMDB files are retained. No application source or API calls change.
`ULMDB_MIGRATOR` can select an alternate migration executable.

Optionally, run an explicit migration before starting the application to control
the import timing or choose the native destination. It reads one consistent
source snapshot and writes a new native environment. The destination must be
absent:

```sh
target/lmdb-ulmdb/bin/ulmdb-migrate /path/to/lmdb /path/to/new-ulmdb
```

After this explicit migration, configure the application to open
`/path/to/new-ulmdb` through the adapter or a native ULMDB client. Opening the
original LMDB path through the adapter instead uses its automatic sidecar import.

A single-file `MDB_NOSUBDIR` source is detected from its regular file path; it can
also be specified with `--nosubdir`. `--library` and `--exporter` select alternate
adapter and exporter builds. `--quiet` suppresses the successful summary.

Main-database user records, named databases, duplicate pairs and persisted
integer/reverse/fixed-duplicate flags are replayed as raw bytes. Binary keys,
values and empty ordinary values are preserved. The importer commits one native
transaction in a private staging directory, then publishes it without replacing
an existing target: Linux uses `renameat2(RENAME_NOREPLACE)`, macOS uses
`renameatx_np(RENAME_EXCL)`, and Windows uses a handle-rooted rename with
replacement disabled. Required directory fences follow publication.
Pre-publication errors clean only the owned stage. If a final fence fails after
publication, the target is retained and an error is returned. An existing target,
even an empty one, is never replaced. The source `data.mdb` is preserved.
`LMDB_MIGRATION.json` records the source snapshot sequence and imported counts.
The metapage format selects the matching pinned LMDB 0.9 or 1.0 exporter. An
`LMDB-ABI` marker preserves its catalog-name convention in the native target;
ordinary LMDB 0.9 catalogs omit the final NUL, while LMDB 1.0 catalogs include it.
The source map-size setting is carried into the new environment. The internal
migration v3 stream also carries the actual source page size and maximum key
size; page geometry is configured before the destination is opened. Earlier v2
streams retain their 4096-byte-page/511-byte-key interpretation. The first
binary database section is MAIN; subsequent sections are named, including an
empty name in modern LMDB. Native logical dumps use version2 explicit MAIN/NAMED
section kinds; version1 input remains supported with its original empty-name
meaning of MAIN. Restoring a modern empty named database requires the
`ULMDB_LMDB_ABI=1.0.2` profile; legacy profiles reject it. Reader-table
capacity is initialized from the application's options when it opens the
published target, rather than being fixed by the private import process.

The importer supports up to 65,528 named databases plus MAIN. Keys and duplicate
values must fit the actual source and destination key limits. Ordinary values
can reach `UINT32_MAX` (4,294,967,295 bytes) on supported 64-bit profiles, subject
to map, transaction, storage and address-space limits. Values over 1 MiB are
spooled privately and copied into `MDB_RESERVE` storage in bounded blocks;
there is no separate 64 MiB importer ceiling. The importer uses a mapped writer,
not an input-sized heap allocation.

The migration exporter runs in a separate process to avoid colliding `mdb_*`
symbols. The Unix exporter child removes inherited `LD_PRELOAD` and, on macOS,
dyld injection settings; Windows starts its separately linked exporter with an
explicit binary pipe and inherited-handle allowlist. The deployed importer is a
native C executable and uses its adjacent exporter and adapter library. The
upstream checkout, C compiler and Cargo are only needed to build the package;
the native packager uses Make, and the Windows tools builder uses Python.

Custom comparator function pointers and application relocation callbacks are not
persisted by LMDB and cannot be recovered from its files. Applications using
custom comparison must continue registering their callbacks when opening the
migrated database. Comparator-specific interpretation of records remains the
application's responsibility. Native ULMDB resource limits still apply; an import
that exceeds them fails without publishing the destination.

After switching to native ULMDB storage, operate that environment through the
adapter or a ULMDB client. Do not concurrently keep writing the preserved original
with the original LMDB engine: its files are a separate historical snapshot.
ULMDB's native files are not an LMDB-format write-through mirror.

## Tests

```sh
make -C clients/lmdb-ulmdb test
make -C clients/lmdb-ulmdb parity
```

The C test runner compiles one application once against the original upstream
header and real shared library. The exact same executable then runs with the
ULMDB adapter selected by the launcher and must remain byte-for-byte unchanged.
Other tests migrate real LMDB main, named, duplicate, integer, reverse and fixed
records, verify them through unchanged LMDB API calls, check source preservation,
and check target preservation and failed-export cleanup. Both LMDB 0.9.31 and
1.0.2 binaries/data files are exercised, including automatic directory and
single-file sidecar import. The suite also rejects malformed exporter streams
and verifies automatic conversion with an empty executable search path, where
Python and shell commands are unavailable.

These tests do not modify performance experiments, measurements, engine formats,
or the default Rust engine's publication strategies.

## Existing Python `lmdb` applications

The original `python-lmdb` package can use this adapter when its extension links
to a shared LMDB library. Its official
[installation guide](https://lmdb.readthedocs.io/en/release/#installation-unix)
documents `LMDB_FORCE_SYSTEM`, `LMDB_INCLUDEDIR`, `LMDB_LIBDIR` and `LMDB_PURE`.
For example, to build an extension against the adapter instead of bundling LMDB:

```sh
LMDB_FORCE_SYSTEM=1 LMDB_PURE=1 \
  LMDB_INCLUDEDIR="$PWD/target/lmdb-ulmdb/include" \
  LMDB_LIBDIR="$PWD/target/lmdb-ulmdb/lib" \
  python3 -m pip install --no-binary=lmdb lmdb
target/lmdb-ulmdb/bin/convert-lmdb-to-ulmdb python3 existing_application.py
```

The application continues to use `import lmdb`, `lmdb.open`, transactions and
cursors. Already dynamically linked extensions can switch through the launcher
without rebuilding. Statically bundled wheels require the extension build above;
the application source does not change.

An additional integration test uses an extension built against real LMDB 0.9.31,
then runs one unchanged application and that same extension against each engine:

```sh
python3 clients/python/compatibility/test_python_lmdb.py \
  --python /path/to/python-with-shared-lmdb \
  --reference-lib /path/to/original-lmdb/lib
```

It verifies executable-extension hashes, automatic migration, binary/empty
values, borrowed buffers, duplicates, integer keys, fixed duplicates, ordered
cursors, snapshots and persisted updates. `python-lmdb` 3.0.0 was exercised with
this setup. These Python-specific fixtures live with the
[Python client](../python/README.md); the optional test does not download or
install dependencies and is separate from the native adapter test suite.
