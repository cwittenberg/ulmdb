# Exact lookup strategy experiment

This standalone crate compares actual prepared ULMDB `KvRead::get` with a
benchmark-only immutable `HashMap<Box<[u8]>, &[u8]>` side index. The hash index
borrows the value bytes from the same pinned snapshot; copied keys and the table
are additional memory. Ordered scans delegate to the original snapshot cursor.
It changes no engine code, default, persistent format or publication procedure.

```sh
cargo build --release --locked --offline --manifest-path benchmark/lookup-strategies/exact/Cargo.toml
taskset -c 4 benchmark/lookup-strategies/exact/target/release/ulmdb-exact-strategy-benchmark \
  --rows 100000 --queries 200000 --samples 7 --backend resident --keys hostname \
  --directory /tmp/fresh-ulmdb-hash-experiment > /tmp/ulmdb-hash-experiment.jsonl
```

Choose an available CPU and fresh directory. `--backend legacy|resident`,
`--keys fixed16|hostname`, `--rows`, `--queries`, `--samples` and `--seed` are
configurable. The seed accepts a nonzero decimal `u64` and is printed in full.
The Resident build selects the feature set used by the existing million-key
benchmark. Its CompactV2/Borrowed loader uses 1,000-row transactions; Legacy uses
sorted bulk loading. Fixture load costs are disclosed but are not equivalent
loader comparisons. Resident capacities scale with rows and are printed.

Queries are generated before timing using a fixed seed: uniform hits, uniform
misses, 50% hits/misses and 90% hits to the hottest 1% of keys. All values contain
32 bytes. Every record and every query is verified against the source fixture
before timing, including equality of returned borrowed value pointers. Each
strategy is warmed once; measured order alternates each sample. Native backend
selection happens before timing, and root binding happens before all queries.
These are warm single-thread throughput costs, not individual request latency,
cold-cache, concurrent writer or larger-than-memory measurements. Output is
JSON Lines with raw sample times, operation counts and result checksums.

Three full hash builds time ordered cursor enumeration, table allocation,
copying every key and inserting all entries. Build 1 includes a fresh randomized
standard hash seed; every later build also receives a fresh seed. Repeat fresh
processes to observe seed/system variability. The reported memory estimate
assumes std's SwissTable bucket layout and adds exact copied key payload. It
excludes allocator metadata/rounding, source mappings and the cost of retaining
old snapshots; it is not measured RSS or a bound on total process memory.

The harness checks 100-record ordered scans through the fallback and holds the
old snapshot/index while publishing an update. It rebuilds the new index and
checks every old and new value. This reports the full-rebuild cost for a single
changed record, exposing the cost a future publication integration must manage.
No transaction-path regression can be inferred from the warm lookup timings:
the candidate is not integrated into writes or recovery. Invalid-key errors,
custom comparators, duplicates, persistence and cross-process index publication
are outside this valid-key experiment and need qualification before adoption.
