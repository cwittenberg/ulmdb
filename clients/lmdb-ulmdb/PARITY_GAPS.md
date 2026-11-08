# Remaining LMDB parity gaps

Current source implements all twelve originally missing extension APIs. It also
implements native comparator indexes, mapped reserve, fixed-address mappings,
chunked reads and automatic file maintenance. The remaining boundaries below
concern storage contracts, explicit resource profiles, backend coverage and
qualification. They are not a list of absent symbols.

The [feature matrix](features/STATUS.md) separates implementation from qualified
support. The [implementation plan](../../LMDB_PARITY_PLAN.md) preserves the original
audit and milestones. Neither replaces the historical
[common conformance receipt](conformance/receipt.json), the scoped
[feature qualification](conformance/features/m3-f02-f03-qualified/README.md), or
the [M3 artifact/performance record](features/M3_IMPLEMENTATION.md). In particular,
provider `9e64` retains its scoped qualification; the later `002d499c` functional
passes and failed performance budgets remain historical evidence. New source
work does not inherit their acceptance or establish full LMDB parity.

| Area | Current implementation | Remaining boundary |
| --- | --- | --- |
| Modern APIs and ABI selection | Preparation, rollback, incremental dump/load, page-size selection, checksum/encryption callbacks and module load/unload/setup are implemented. Packages provide pinned upstream headers and explicit runtime profiles for 0.9.31, 0.9.36 and 1.0.2. | Profile selection is explicit; the persisted ABI marker has priority. `MDB_VL32` is unsupported. Broad modern/backend qualification remains open. |
| Preparation and rollback | The pinned 1.0.2 Linux Legacy-backed C profile has scoped qualification. Pending transactions retain unpublished state and permit final commit/abort; eligible rollback preserves authenticated predecessor data and old readers. | This is not restart-recoverable distributed two-phase commit. Rollback requires the documented exclusive-use contract and is not constant time. Range and replica rollback remain unsupported. |
| Previous snapshots and copies | Native/C previous-snapshot roles, genuine D/P copies, compact copies and strict v2 FD archives are implemented. | PREVSNAPSHOT belongs to the pinned 1.0.2 profile, not the older headers. Unavailable history and reference corrupt-copy roles have explicit safe `ENOTSUP` boundaries rather than corrupt output emulation. Full G03 acceptance remains open. |
| Incremental snapshots | The three C incremental APIs stream and replay exact retained native bases, validating staged D/P before publication. Caller-managed NOLOCK variants are implemented. | Streams are native deltas, not LMDB raw-page deltas. Expired or mismatched bases fail explicitly; PREVSNAPSHOT source deltas are unsupported. The C loader retains its writable WRITEMAP contract. |
| Ordering, append and DBIs | New native indexes implement built-in/custom ordering, indexed append checks and direct mapped reserve. Transaction-local aliases preserve requested names, callback authority, independent private roots and version-specific MAIN-clear behavior. | Older byte-ordered adapter files with alternate-order META use the explicit scan fallback. Applications must register the same custom comparator on each use; callbacks are not persisted. Broader G07/G14 acceptance remains open. |
| Page geometry and statistics | File formats support explicit power-of-two pages from 256 through 65,536 bytes. Checked graph walks report actual native depth/pages, duplicate inner trees and overflow allocation. | Statistics describe native decoded allocation, not identical LMDB page layouts, ciphertext bytes or RSS. Statistics are graph traversals. Range retains 4 KiB pages. Logical map quotas do not reproduce every LMDB physical `MAP_FULL` threshold. Begin/renew/writer entry now reports `MAP_RESIZED` for actual footprint growth, including Native-only writes; a size hint alone does not reject a fitting snapshot. `mapsize=0` adopts capacity with a checked minimum clamp, preserving old borrows/reset/slots. Exact-checkpoint scalar caching can still require one metadata walk for a new view; resource/I/O errors fail rather than certify a size. |
| Values, keys and catalogs | Default file profiles remain 64/128 MiB. Explicit LARGE permits 512 MiB values; the 64-bit MAXIMUM profile permits 4,294,967,295 bytes with larger storage envelopes. File/Resident catalogs support 65,536 slots; C reserves eight and permits 65,528 named databases. | Growth requires actual map-capacity admission and supported host/profile limits. C key/duplicate limits follow the selected page geometry/profile. External database names use that profile's key bound: legacy default 511 bytes; modern subtracts its stored NUL terminator. Modern empty named databases remain distinct from MAIN; legacy profiles reject empty names. Resident KV/IP values remain 16 MiB and duplicates 64 KiB. Aggregate memory/capacity acceptance is workload-specific. |
| FIXEDMAP, WRITEMAP and MAPASYNC | Genuine address reservations, writable allocation, asynchronous mapped flushing and authenticated checksum-only fixed backing are implemented on Linux/macOS/Windows source paths. Reusable slots use Linux v2 or full 128-bit M3 with dedicated leases on macOS/Windows. | These are native layouts, not an LMDB single-map file. Occupied bases fail without clobbering foreign mappings. REMAP_CHUNKS/encryption with FIXEDMAP is rejected. Existing v1/v2 wire bytes are retained; caller-managed NOLOCK uses append-only v1. Host runtime and broader resource acceptance remain open. |
| Chunked reads and initialization | REMAP_CHUNKS uses bounded authenticated windows whose borrowed views retain ownership. Cold scalar header checks use short scopes. NOMEMINIT skips zero stores only in never-exposed fresh mapped suffixes; reused ranges and heap values remain initialized. | Fully pinned cache exhaustion fails explicitly. Cache/decode/descriptor limits remain real limits. NORDAHEAD applies platform advice on Linux/macOS and is accepted and ignored on Windows, matching pinned upstream behavior. |
| Durability and NOLOCK | Real data/metadata policies, transaction-local weakening, selected-file forced sync and mapped async debt are implemented. NOLOCK bypasses native writer/Capture/History/reader/pin coordination through explicit external authority. | MAPASYNC alone retains Full policy; asynchronous data debt clears only through the required forced flush. NOLOCK callers serialize all operations and maintenance. Focused I/O checks are not universal power-loss or performance qualification. |
| Automatic maintenance and history | Ordinary coordinated Linux/macOS/Windows file owners enable bounded incremental maintenance. Failed optional rewrites preserve the ready business candidate; abandoned preparations are discovered safely. Exact historical metadata is archived before obsolete payload deletion. | KeepAll is the default and deliberately does not bound historical metadata. Explicit finite policies expire eligible bases while live/named pins clamp expiry. Discovery/inventory costs and oversized typed nodes retain admission/defer boundaries; stable live data alone is not a universal total-space bound. |
| Resident and Range | Resident implements nested savepoints, duplicates, persisted ordering, WAL rotation/growth and negotiated independent read-only capture on all three platform source paths. Default-off Range6 implements typed retention, compaction, pack rotation and leased reclamation. | Resident remains an explicit bounded backend; atomic snapshot export source now covers Linux/macOS/Windows, with other-host runtime qualification still open. Replication remains in development. Range5 refusals remain, and Range is not an adapter backend or a fully qualified replacement/replication format. |
| Files, permissions and migration | Native files preserve ULMDB identities and storage contracts. Linux mode/umask and explicit SGID shared-group policy are implemented. Read-only source conversion uses an isolated exporter and an atomic native import. | FD copies are strict native archives, not raw LMDB page files; `mdb_env_get_fd` exposes native state. Initial conversion needs a writable destination. Updating the original LMDB files later does not update the imported copy. Other-host permissions are not a claim of POSIX group/ACL equivalence. |
| Tools and platforms | Binary-safe dump/load, spooled large fields, reader cleanup, JSON diagnostics, native delta pipes and writer-concurrent Native read/check/copy commands are implemented. Native dump v2 explicitly distinguishes MAIN from an empty named database and still reads v1; migration v3 preserves that distinction by section ordinal. Windows packages now link the operator, importer, launcher, both pinned exporters and adapter DLLs. Darwin tool/dyld source is integrated; its Rust C static library release build passes. | No Apple C SDK/compiler build or Windows/macOS runtime journey is claimed here. Logical dumps omit native IP indexes and application crypto callback setup; custom callback code/keys are never serialized. Broader packaging, host, crash and performance acceptance remain open. |

The map-size behavior has a permanent Native-peer regression in
[map_size.rs](../c/src/map_size.rs) and retained local
[same-executable pinned-reference evidence](../../target/qualification/m38-c-map-resized-work/final-replay-v1/receipt.json).
That focused replay precedes the additional creator-PID guard and supplies no
constant-time, full-performance or other-platform acceptance.

The focused Linux M5 witness uses a 256 KiB WAL and 64 slots across four redo
modes, preserving old borrows and restarts through more than ten WAL capacities.
Phase-boundary disk accounting includes deleted-open assets: current Root observes
438,272 allocated bytes maximum and 1,720,320 bytes RSS/high-water growth within
predeclared bounds; allocated assets after old-reader release are 274,432 bytes,
and owner close leaves zero asset FDs. Existing
[checkpoint/resource](../../crates/ulmdb/tests/resident_checkpoint.rs),
[checkpoint fault](../../crates/ulmdb/tests/resident_checkpoint_faults.rs), and
[export](../../crates/ulmdb/tests/resident_export.rs) tests preserve this scope,
including 36 fail/death cuts and 18 real I/O cases. These checks do not establish
transient peak disk usage, universal resource bounds or complete M5 acceptance.

Native ULMDB files intentionally differ from LMDB files. Automatic conversion
preserves the originals; exact LMDB file-format identity is not the migration
contract. Default `Environment` and the C adapter use file-backed storage;
Resident and Range are separate explicit backends with their own limits.

The pinned header describes relocation callbacks as unimplemented upstream.
Accepting that callback does not promise pointer relocation or make an arbitrary
application pointer persistent across native copies or address changes.

Historical differential tests and the current focused source witnesses are
empirical checks, not a proof of every permitted call sequence, filesystem,
interruption or operating environment. Canonical feature states, counts and
receipts remain unchanged. Run the recorded common profile from the repository
root with `make -C clients/lmdb-ulmdb parity`; the
[build guide](README.md#build) documents pinned prerequisites and packaging.
