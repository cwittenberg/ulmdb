# ULMDB's LMDB API adapter

The C adapter implements LMDB's byte-oriented C API on native ULMDB transactions.
Applications keep their `lmdb.h` calls. The [lmdb-ulmdb package](../lmdb-ulmdb/README.md)
also supplies LMDB library names so dynamically linked applications can select
ULMDB without editing or relinking their executable. Static or privately bundled
LMDB bindings must be rebuilt against the adapter; application calls stay the same.

The compatibility target is the public LMDB 0.9 API, with selected 1.0 additions.
Tests compile unchanged programs against upstream headers, run the same dynamic
executable with each library, and exercise an unchanged public Python `lmdb`
application. This does not establish arbitrary LMDB implementation or raw-page
compatibility, nor qualify performance or durability stages.

See the [functional conformance results](../lmdb-ulmdb/conformance/REPORT.md) for
the tested profile and the [remaining parity gaps](../lmdb-ulmdb/PARITY_GAPS.md)
for unsupported features and behavioral differences.

## Build and use

```sh
cargo build -p ulmdb-c --locked
make -C clients/lmdb-ulmdb
./target/lmdb-ulmdb/bin/convert-lmdb-to-ulmdb your-existing-application
```

The package includes `liblmdb.so.0`, `liblmdb.so`, `liblmdb.so.1`, `liblmdb.a`,
headers, a launcher and migration tools. It currently targets Linux. The direct
Rust build also produces `target/debug/libulmdb_c.so` and `libulmdb_c.a`.
For a source build, keep the existing include and `-llmdb` linkage and select
`target/lmdb-ulmdb/include` and `target/lmdb-ulmdb/lib` through the build system.
For a dynamic executable, the launcher selects the adapter's library directory.
No system library is overwritten.

## Automatic maintenance

Ordinary coordinated Linux file owners start with automatic maintenance: a
128-segment trigger, a 16 MiB rewrite allowance and up to eight file unlinks per
commit. Legacy, Cohort, Indexed, cached and writable-map owners retain progress
between native writer lifetimes. A business write is prepared before optional
rewriting, so optional preparation failures preserve the ordinary write. Fresh
owners also scan recognized abandoned preparation files under live-root protection.
Historical cohort metadata is archived before payload retirement. Oversized
opaque values keep their containing segment while other objects can move.

With no active transactions, `ulmdb_env_set_maintenance` in `ulmdb.h` changes the
process-local policy; a zero segment threshold disables it. NOLOCK owners retain
caller-managed maintenance. Fixed-map layout versions and other platforms have
their documented boundaries.

Historical incremental bases remain available by default. Explicit finite
retention is configured with the Native API or `target/release/ulmdb history-policy
PATH N`; `all` restores KeepAll for future expiry decisions. Keep a named snapshot
pin for a closed backup needed as a future delta base. Live and named pins clamp
expiry, and expired bases return an explicit error. `history-status PATH` reports
the floor; `maintenance-step PATH` performs bounded idle work without a commit.

## When conversion is needed

A separate manual conversion step is optional:

- New databases and existing native ULMDB databases need no conversion.
- Existing LMDB data must be imported into ULMDB's native format. The packaged
  adapter does this automatically on the first open, preserves the original
  files and reuses the native copy on later opens.
- Use `ulmdb-migrate SOURCE DESTINATION` if you want to import ahead of time or
  choose a separate destination. Then point your application at that destination.
  See the [migration instructions](../lmdb-ulmdb/README.md#migrate-an-existing-lmdb-environment).

`convert-lmdb-to-ulmdb APPLICATION [ARGUMENT ...]` is an optional application
launcher. It selects the ULMDB adapter for that run. Use it each time you start
the application unless your library search path or build already selects the
adapter. Conversion happens only when the adapter opens existing LMDB data that
has no native copy yet.

## Implemented behavior

| Area | Behavior |
| --- | --- |
| Transactions | Read-only snapshots, serialized writers, nested savepoints, commit/abort, read reset/renew and `MDB_NOTLS` sequential thread transfer. Parent commits finish live children. |
| Processes | Independent processes can open the same native environment. Readers capture committed immutable data without waiting for a writer; writers hold a process lease until publication and owner cleanup finish. |
| Readers | Shared reader slots, configured limits, retained slots across reset/renew, reader listing and dead-reader cleanup. The reader table belongs to the adapter. |
| Databases | Main and named databases, transactional creation/drop, stable DBI identities, a main-cursor catalog, sorted duplicates and persistent ordering/width flags. Main DBI is 1. |
| Ordering | Unsigned byte, reverse-key, native-endian integer, reverse/integer duplicates, fixed-width duplicates and application comparison callbacks. Register custom callbacks in every process, as with LMDB. |
| Writes | Overwrite/no-overwrite, no-duplicate-data, ordered appends, writable reserve buffers, multiple fixed-size duplicate insertion and cursor replacement, including valid size-changing duplicate replacement. |
| Cursors | First/last, next/previous, exact/range and duplicate seeks, deletion/counting, renewal and bounded fixed-duplicate batches. Ordinary data reads return borrowed native bytes. |
| Environment controls | Directory and `MDB_NOSUBDIR` paths, read-only operation, runtime durability flag changes, synchronization, context/assertion registration, native descriptor access and logical map-capacity admission. |
| Copies | Independent native directory copies, compacted copies and bounded archive streams through caller-owned descriptors. The adapter reopens those archives through `MDB_NOSUBDIR`. |
| Incremental backup | `mdb_env_incr_dumpfd`, `mdb_env_incr_dump` and `mdb_env_incr_loadfd` stream, validate and atomically select native deltas from an exact retained base backup. |
| Page geometry | `mdb_env_set_pagesize` selects a persisted power-of-two tree page size from 256 through 65536 before open. |
| Checksums and encryption | `mdb_env_set_checksum` and `mdb_env_set_encrypt` install actual file-storage callbacks before open; `mdb_modload`, `mdb_modsetup` and `mdb_modunload` support native crypto modules. |
| Physical statistics | Native tree depth/pages and deduplicated overflow-page spans from checked graph traversal. MAIN entry counts include named descriptors without synthesized physical pages. |
| Large values | File profiles support 64 MiB, 128 MiB, 512 MiB and, on 64-bit hosts, exactly 4,294,967,295 bytes per value. C requests require sufficient configured map capacity; rejected logical flag checks do not upgrade the profile. |
| Large catalogs | Explicit `mdb_env_set_maxdbs` requests can select the persisted 65,536-slot native profile, with up to 65,528 named databases and eight internal slots. Older readers reject that profile. Ordinary read-only MAIN cursors share a snapshot-local catalog index and return pinned descriptor views. |
| IP additions | Transactional IPv4/IPv6 prefix insertion, exact lookup, deletion and longest-prefix matching through `ulmdb_*` functions. |

Buffered-file commits now apply `MDB_NOSYNC` and `MDB_NOMETASYNC` to native data
and metadata fences, including supported transaction-local overrides in the
modern API profile. The default remains synchronous; `mdb_env_sync(force)`
flushes the selected native files. `MDB_MAPASYNC` alone does not weaken buffered
publication. `MDB_WRITEMAP` now allocates writer data and reserve buffers in a
real shared writable file mapping. Without transforms, Legacy/Cohort/Indexed
seal and publish the same inode after revoking the writable map; `MDB_MAPASYNC` performs asynchronous mapped
flushes of the completed persisted object and retains weak-publication debt until
forced sync. Checksummed writers encode the finished data and asynchronously
flush that encoded object. Encrypted opens select their separate decoded cache
path. Nested C writers are rejected in writable-map mode. Fixed and writable
mapping can be combined on Linux. The
[durability implementation record](../lmdb-ulmdb/features/G04_IMPLEMENTATION.md)
distinguishes current source, functional/fault results and incomplete performance
and power-loss qualification. On Linux, `MDB_NORDAHEAD` applies `MADV_RANDOM` to
the admitted native mappings. With `MDB_WRITEMAP`, `MDB_NOMEMINIT` skips
redundant stores into fresh zero-backed mapped space. Exposed ranges are tracked
across truncation and rollback, so reused bytes are still cleared; heap writers
retain initialized allocation. Runtime changes apply before the next mutation.
`MDB_NOLOCK` is for applications that arrange their own access;
it now bypasses the environment, writer, native ownership, reader and maintenance
coordination locks. Callers serialize all operations and maintenance externally;
do not mix it with independently coordinated writers. Reads, prepared writes,
previous-snapshot opens, copies, incremental replay and rollback use the same
external policy. The empty `LOCK` file is a permission-policy anchor and is never
locked in this mode.

With `MDB_WRITEMAP | MDB_MAPASYNC`, `mdb_env_sync(env, 0)` schedules asynchronous
flushes of selected files without clearing durability debt. Passing `1` performs
the full synchronization. `MDB_NOSYNC` skips a nonforced call.

New databases persist integer, reverse and custom ordering in their native indexes.
Older adapter databases whose alternate ordering exists only in adapter metadata
retain compatibility scans. Reserve buffers point into initialized writer-owned
value storage and remain valid until the next update, as required by the C API.

Incremental backups require the exact native base checkpoint, including retained
history. They use a bounded native delta format; compacted copies and LMDB page
files are different formats. Loading requires exclusive access and the documented
`MDB_WRITEMAP` or `MDB_REMAP_CHUNKS` open mode. Encryption implicitly selects
`MDB_REMAP_CHUNKS`, so encrypted bases can load deltas without a writable
plaintext mapping. Caller-owned file descriptors stay open.

Checksum/encryption callbacks protect file data, catalog and overflow payloads,
commit descriptors and indexed metadata. Fixed `CURRENT`/`PREVIOUS` selectors
remain clear CRC records. Encryption copies the supplied key and passes the
callback a key/16-byte IV/authentication tuple. Checksum and authentication widths
are bounded at 256 bytes; keys at 1 MiB. Transformed objects use immutable decoded
mapping leases with an explicit memory quota. `MDB_REMAP_CHUNKS` now uses bounded
pinned windows throughout reads, writes, copies, archives and incremental replay;
Native cached sources share a bounded descriptor pool (32 by default) and reopen authenticated rooted files instead of retaining one descriptor per segment. Larger cache windows require an explicitly larger map capacity; pinned exhaustion returns an error. Exact LMDB resource parity remains open. Keep a loaded crypto module alive until all environments using its hooks
have closed. Key derivation failure is reported by the subsequent environment
open. Resident WAL and checkpoint transformations are implemented through its
Native API; broader platform qualification remains open.

The optional [OpenSSL AES-256-GCM module](crypto/README.md) supplies real
encryption through the `MDB_crypto` hooks. Build it with
`make -C clients/c/crypto`; it is separate from the engine build.

Run the focused extension round trips with
`python3 clients/c/tests/extensions.py --package target/lmdb-ulmdb` after building
the package. The [feature inventory](../lmdb-ulmdb/features/STATUS.md) separates
these implementations from the remaining parity work and qualification.

## Storage and compatibility boundary

Native file owners can opt into bounded automatic maintenance. Quota-limited
rewrites defer to the original write. C writers retain incremental history by
default; automatic history reclamation requires a separate retention contract.

Native ULMDB files are different from LMDB page files. Existing LMDB 0.9 and
1.0 snapshots can be imported by the packaged tools. Opening an existing LMDB
directory through the packaged adapter creates a native `.ulmdb` sidecar;
`MDB_NOSUBDIR` files use a sibling `.ulmdb` directory. The original files are
preserved. Subsequent adapter opens use native storage at the same application
path. Switch all participating processes together: an original LMDB library
continues to see the preserved original snapshot, not later ULMDB writes.

Applications that directly parse LMDB pages, depend on its contiguous map or
persist raw mapped pointers require a different storage compatibility mechanism.
On Linux, experimental `MDB_FIXEDMAP` reserves a persistent address window and
maps native immutable files into authenticated slots. `MDB_FIXEDMAP | MDB_WRITEMAP`
also places actual writable reservations in that window. An occupied recorded
base fails without replacing the occupying mapping (`MDB_ADDR_BUSY` in the
modern profile, `EBUSY` in the legacy profile). Chunk remapping/encryption with
fixed placement returns `EINVAL`. Checksum-only fixed placement uses authenticated
sealed decoded backing. Checksummed `MDB_WRITEMAP | MDB_MAPASYNC` schedules
the finalized encoded object and retains data debt until forced sync. Native
required-locked fixed mappings charge the shared physical slot until its actual
unmap, including aliases retained by demand readers. This is native multi-file placement, not LMDB
page-file compatibility. `MDB_envinfo` map/page
fields and `MDB_stat` physical-page fields describe native measurements rather
than an LMDB page layout. `ms_entries` counts logical records and duplicate pairs.
`mdb_env_get_fd` exposes a native publication descriptor; use the copy functions
for a complete independent environment.

Close named handles before clearing the main database for portable behavior.
The current typed alias model preserves pinned legacy cached-handle behavior
through retained native views. Explicit 1.0 catalog mode rejects open named
handles with `MDB_DBIS_BUSY`, as upstream 1.0 does. The
[previous-snapshot and alias record](../lmdb-ulmdb/features/G03_IMPLEMENTATION.md)
documents the reference profiles and current qualification limits. Clearing
fixed/integer databases resets their learned widths.

The adapter enforces a logical capacity budget for `mdb_env_set_mapsize`,
including record and catalog overhead. Its thresholds need not match LMDB's page
packing. Ordinary opens do not reserve a contiguous virtual map; fixed opens
reserve the separate bounded address window described above. A capacity failure
returns `MDB_MAP_FULL` and requires aborting the failed writer before retrying.
Native ordinary values currently have a 64 MiB limit; keys and duplicate values
are 1–511 bytes. Database names use adapter aliases when native name limits are
smaller. Native limits and the separate journal-backed Range pilot are not
expanded by this compatibility layer.

The Linux C process protocol coordinates adapter users and registers immutable
snapshots with native capture leases and physical pins used by coordinated
maintenance. This does not make arbitrary external file changes safe. Its
independent snapshot entry point supports Legacy/Cohort/Indexed files;
journal-backed pilot recovery retains its own API. Caller-managed `MDB_NOLOCK`
access still requires external serialization, and other backends/platforms
require separate qualification.

## Ownership and resource bounds

A transaction owns returned `MDB_val` views until its next update, reset/renew or
end. Cursor movement and close do not invalidate ordinary borrowed record views.
Never free those buffers. Fixed-duplicate packed batches are at most one logical
page and are retained as returned buffers until the transaction ends. A formatted
CIDR from `ulmdb_longest_match` is scratch text valid until the next longest-match
call or transaction end; copy it when needed longer.

Ordinary cursor ordering uses native bounded seeks and keeps only its current
position. Comparator/reverse/integer strategies may scan the native index while
retaining one candidate; they do not materialize all records. Returned virtual
catalog descriptors and packed batches are retained only when requested. There
is no claim of LMDB performance parity for these adapter strategies.

The source uses small modules:
the C ABI is the Facade, path/migration handling is an Adapter, cursor ordering
uses a Strategy, and borrowed seeks implement Iterator traversal. Process and
snapshot leases use RAII. Modules separate ordering, names, catalog, capacity,
paths, copies, archive validation and process coordination; the 3,000-line limit
applies to every first-party source file.

## Tests and examples

```sh
./clients/test.sh
make -C clients/lmdb-ulmdb test
```

The differential runner checks upstream source hashes before compiling unchanged
C scenarios against pinned LMDB. Additional tests cover processes/crashes,
reader resources, streaming memory bounds, malformed archives, migration,
unchanged dynamic executables and cross-language data exchange. See the
[adapter package](../lmdb-ulmdb/README.md) for the optional public `python-lmdb`
compatibility test and its external prerequisites.

[The ordinary C example](examples/basic.c) uses the LMDB byte API.
[The IP example](examples/routes.c) includes `ulmdb.h` for native IPv4/IPv6 calls.

```sh
cc -std=c11 -Wall -Wextra -Werror -I clients/c/include \
  clients/c/examples/routes.c -L target/debug -lulmdb_c \
  -Wl,-rpath,"$PWD/target/debug" -o /tmp/ulmdb-c-routes
/tmp/ulmdb-c-routes /tmp/new-ulmdb-c-routes
```

Parent directories must exist. Newly initialized environments use owner-only
Unix directory permissions (`0700`); the LMDB file-mode argument does not weaken
that native directory policy.
