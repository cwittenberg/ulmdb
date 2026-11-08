# Explicit lookup strategies

The snapshot-scoped hash lookup is implemented for managed, untransformed,
bytewise, single-value KV databases in both Legacy and Resident. Callers choose
it explicitly; ordinary readers and writers do not construct an index. The hash
and the native ordered fallback borrow the same transaction. No automatic
selection, persistent index, writer hook or background rebuild is installed.

**Performance admission is conditional.** Exact lookups improve materially,
including against native LMDB, but the measured no-regression gate has not
passed. The complete tables retain slower scans, sampled latency observations,
build/rebuild cost, memory, old/new overlap and durable writes. Do not infer
universal parity from point-read improvements.

```rust
use ulmdb::lookup::{HashLimits, Options};

let read = environment.begin_read();
let lookup = read.prepare_lookup(b"hosts", Options::Hash(HashLimits {
    max_entries: 100_000,
    max_index_bytes: 16 * 1024 * 1024,
    max_value_bytes: 4096,
    max_total_value_bytes: 128 * 1024 * 1024,
}))?;
let value = lookup.get(b"example.net")?;
```

`Options::Ordered` delegates to the established prepared reader without
enumerating the database. `lookup.cursor(lower, upper, reverse)` retains ordered
range semantics. See the runnable [example](../../crates/ulmdb/examples/lookup_strategy.rs)
and [API](../../crates/ulmdb/src/lookup.rs).

Hash construction performs two complete scans. Keys and values are borrowed;
only the slot vector is owned. `hash_index_bytes(entries)` reports required slot
bytes on the current architecture. Limits reject oversized input or allocation
failure with `LimitExceeded`, without returning a partial index. Index-byte
limits exclude allocator bookkeeping, native cursor workspace and every page
retained by the caller's transaction. Budget old and new indices separately if
both generations remain alive. Dropping the lookup does not drop a transaction
that the caller still owns.

Hashing rejects duplicate/prefix/Range databases, non-bytewise comparators,
external mappings and transformed storage. The ordered strategy retains native
admission rules. Lifetimes prevent the prepared lookup from escaping its read
transaction; descriptive commit/root identity is not a reusable cache token.

## Completed implementation and LMDB comparison

- [Qualification and full comparison overview](completed/REPORT.md)
- [Every direct read, scan, write, lifecycle and memory result](completed/LOOKUP_FULL_TABLE.md)
- [Machine-readable full table](completed/LOOKUP_FULL_TABLE.csv)
- [All default-path results, primary campaign](completed/DEFAULT_PRIMARY.md)
- [All default-path results, nine-pair confirmation](completed/DEFAULT_CONFIRMATION.md)
- [Full default-path comparison with the same build directory](completed/DEFAULT_CANONICAL.md)
- [Primary versus confirmation, every throughput and sampled p99 cell](completed/DEFAULT_REPEATS.md)

The direct campaign runs original native KvRead, the actual production hash API
and pinned native LMDB in separate fresh processes. Each arm checks complete
initial, old, new and reopened bytes, bounds, direction and timed checksums.
LMDB uses normal synchronization and locking, with NOTLS to allow the held old
reader and same-thread writer. Both ULMDB native and hash arms retain an old
snapshot during the pinned write phase; the hash arm additionally owns its
index. Construction, an explicit new-generation rebuild and dropping both
indices are charged separately. Resident's benchmark capacity and charged
pre-pin Collect are documented in the full table; engine defaults are unchanged.

The default-path campaign compares frozen before/after sources with the lookup
unused, including IPv4, IPv6, ordered KV and durable writes on disk and tmpfs.
Both the primary campaign and its independent complete repeat remain available.
The measurements use warm synthetic data on one pinned core of a shared host;
request tails, cold storage, real OS memory pressure and application throughput
need their own qualification. A controlled allocator-refusal probe is a
separate failure-safety check, not a memory-pressure performance claim.

Reproduce the direct campaign against a deliberately frozen source tree:

```sh
python3 benchmark/lookup-strategies/direct_campaign.py \
  --source /path/to/frozen/source --output benchmark/work/NEW_DIRECT \
  --cpu 12 --rounds 3 --sizes 8192,100000,1000000 \
  --queries 65536 --samples 7 --write-transactions 4
```

`--build-only`, followed by `--resume` with identical options, separates
compilation from timing. Source, executable, configuration and raw-result
hashes must match. See `regression.py --help` and `confirmation.py --help` for
the paired default-path procedure. No existing public benchmark is overwritten.

## Earlier research

The [original 54-process prototype report](results/REPORT.md),
[raw evidence](results/results.json), [qualification](results/qualification.json)
and [archive](results/raw-evidence.tar.gz) are retained as historical evidence.
Those prototypes copied keys and their scan controls already had an index
resident; use the completed production comparison above for admission.

[bbtree](ordered/README.md) remains an isolated helper because its dependency
graph conflicts with ULMDB. The [checked 4-bit prefix candidate](prefix/README.md)
remains experimental: modest large point-query improvements came with substantial
all-covering and smaller-workload losses. Neither candidate is installed in the
transactional core.
