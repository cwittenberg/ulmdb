# Direct completed lookup capability comparison

This benchmark compares the actual public ULMDB `prepare_lookup` capability,
the original native `KvRead`, and a pinned native LMDB build. Each arm runs in its own
fresh process and fresh database directory. The `native` arm directly uses the original prepared reader and never constructs
a hash table, including validation and reopening.

```sh
cargo test --locked --manifest-path benchmark/lookup-strategies/comparison/Cargo.toml
cargo build --release --locked --manifest-path benchmark/lookup-strategies/comparison/Cargo.toml
# Run sequentially on an allowed CPU, while other builds/tests are idle.
taskset -c 20 benchmark/lookup-strategies/comparison/target/release/ulmdb-direct-strategy-comparison \
  --keys 100000 --queries 65536 --samples 7 --seed 73 \
  --arm hash --backend resident --key-kind hostname \
  --write-transactions 4 --dir /tmp/new-comparison-database > hash.json
```

Arms: `native`, `hash`, `lmdb`. Backends: `legacy`, `resident`. Key kinds:
`fixed16`, `hostname`. Values are32 bytes. Dataset sizes are configurable,
including8192,100000 and1000000. `lmdb` ignores the ULMDB backend selection.
Use fresh processes per arm and rotate arm order across repeated processes.
The filesystem containing `--dir` selects disk versus a real RAM filesystem.
An optional `--ballast-mib N` touches and retains identical extra memory in each
arm; this is synthetic resident ballast, not a controlled OS memory-pressure test.

Read cases are exact hit, exact miss, mixed50, hot90, forward/reverse100-row
scans, and forward/reverse64-row bounded ranges. All query results are compared
with complete expected bytes before timing. Cursor bounds are precomputed
outside the timed region. Timed digests include every returned key/value byte
and cursor order. Prepared views bind outside point timing, with equal payload
consumption in all arms.

The source-only and hash arms use the same compiled engine feature profile.
Before taking the long-lived snapshot, Resident explicitly runs its ordinary
`collect()` maintenance; Legacy and LMDB have no corresponding operation here.
The `maintenance_before_pin` lifecycle entry charges this work separately from
index construction. The capability itself never triggers collection. Without
this maintenance, full100K and1M hostname trials exhausted the source page pool
identically for native and hash arms: an old snapshot also pinned unreachable
historical versions still present in its directory. Those failed trials are
retained as counterexamples. A sufficiently long pinned write stream can still
exhaust a finite pool; this benchmark is not a proof of unlimited retention.
After pre-pin Collect, the original39442-map1M geometry also exhausted its pool
near the end of the full pinned write phase: each live hostname generation used
about21853 maps. The final benchmark admits `ceil(keys/24)+8192` reusable maps
and256MiB metadata in both source-only and hash arms, allowing two live
representations plus a1000-record writer transaction. This is declared benchmark
capacity, not an engine or guard change. Actual mapped/RSS/file costs remain
reported; no unlimited-reader or indefinitely pinned writer claim follows.
The native baseline avoids the new Ordered Strategy wrapper.
The hash is the completed bounded borrowed-key/value production strategy,
not the prior copied-key `std::HashMap` experiment. It has a512-MiB owned slot
budget plus explicit entry/value bounds. JSON contains actual slot allocation
statistics separately from observed process memory.

Writer cases use batch1,64 and1000, with normal durable commits and identical
precomputed overwrite mutations. They run before pinning, while an old snapshot
is pinned, and after releasing it. The native arm pins an old read transaction;
the hash arm pins the same transaction plus its index. The LMDB arm uses NOTLS
to permit a same-thread retained reader and writer, while keeping normal file
locking and durable commit flags. The capability never rebuilds automatically
on writes. Every old, new, final and reopened record is checked in full.

Unpinned-before/pinned/unpinned-after controls run sequentially and therefore
include evolving layout/history. Compare the same phase between independent
arms; these sequential controls alone do not isolate the causal cost of pinning.

Lifecycle JSON records environment creation/loading, first preparation,
new-generation rebuilding, individual index dropping, old-snapshot release,
closing, warm-cache reopening and rebuilding after reopen. Initial loaders use
the best currently available explicit path: Legacy sorted bulk load in one
transaction, Resident ordinary puts in batches1000, LMDB sorted puts in one
transaction. These are identified and are not equivalent loader algorithms.

Memory checkpoints record actual Linux VmRSS, VmHWM, VmSize, RSS anonymous/file
components and page-table bytes. Inputs, query vectors, model state, allocator
retention and source mappings are included. VmHWM is cumulative. A final full
validation warms all reachable values, so warm resident comparisons do not
mistake untouched mappings for a smaller working set. File apparent and actual
allocated bytes are recorded separately. This is not a forced eviction test,
crash/recovery qualification, or universal no-regression proof.

Output is one JSON object with `config`, per-sample `samples`, `lifecycle`,
`memory`, `files`, pinned LMDB build provenance, and final validation fields.
Write `operations` count records, read point `operations` count lookups, and scan
`operations` count complete cursor requests. All timing samples remain available.

On a failed run, stderr includes `COMPARISON_PARTIAL` JSON with completed samples,
lifecycle and RSS observations. Writer errors identify pin state, batch, sample,
transaction and source begin/put/commit stage, plus source pool/private-workspace
diagnostics where available. Failed artifacts must remain classified separately
from complete qualifying campaigns.
