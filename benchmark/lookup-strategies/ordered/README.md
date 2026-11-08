# Ordered lookup strategy experiment

This standalone benchmark compares actual `bbtree` **0.2.0** (single-thread
`unsync::BBTreeMap`), Rust's `BTreeMap`, and a prepared reader in the current
public ULMDB Legacy backend. It changes no production code or default algorithm.

Both side indexes own copied keys and store offsets into a vector of borrowed
records. ULMDB and std borrow a pinned read snapshot. The bbtree helper borrows
an exact immutable export of that snapshot, validated byte-for-byte against
the shared deterministic fixture generator. Result values are borrowed, with no
per-lookup copy. The bbtree default inline-key strategy is active for byte keys.
Each candidate handles hits, misses, mixed queries and ordered 64-record scans
over fixed 16-byte keys and generated variable hostname keys. The benchmark
checks every record and every generated missing key against a sorted independent
oracle, plus range result bytes and ordering, before recording samples. An old
index must retain its old values after a later committed update.

```sh
cargo test --locked --manifest-path benchmark/lookup-strategies/ordered/Cargo.toml
cargo test --locked --manifest-path benchmark/lookup-strategies/ordered/bbtree/Cargo.toml
cargo build --locked --release --manifest-path benchmark/lookup-strategies/ordered/bbtree/Cargo.toml
cargo build --locked --release --manifest-path benchmark/lookup-strategies/ordered/Cargo.toml
taskset -c 4 benchmark/lookup-strategies/ordered/target/release/ulmdb-ordered-lookup-benchmark \
  --keys 100000 --queries 100000 --samples 7 --seed 73 --dir /tmp/new-ordered-fixture \
  > /tmp/ordered-results.json
```

The fixture directory must be new and is retained for inspection. The parent
launches the helper by default from `bbtree/target/release/ulmdb-bbtree-worker`;
`--bbtree-worker PATH` overrides it. Both processes inherit CPU affinity. Native
and std samples rotate order after warmup. Bbtree samples run in a separate
process before or after them, alternating by seed/key kind. Helper startup and
IPC are excluded from per-lookup samples and fixture/export costs are separate. Range throughput counts queries, each returning up
to 64 records; it uses at most 4096 range queries per sample. Side-index
build reports snapshot enumeration, key copying and insertion; bbtree also
reports fixture export/read/decode separately. Complete integration cost must
include those process-boundary costs. ULMDB load
cost includes creating the environment, sorted loading and durable commit.

Memory reports requested bbtree node-page bytes, owned key capacities and the
shared borrowed-record vector. Allocator headers and mapping RSS are excluded.
For std's BTreeMap, entry storage is explicitly a lower bound and node overhead
is unknown. These measurements do not establish cold-cache, write throughput,
cross-thread performance, bbtree durability or transaction equivalence. Future
integration would need snapshot-bound publication, update/rebuild costs and
real application workloads assessed separately.

The separate helper is necessary because bbtree 0.2.0 pins `vstd` and
`verus_builtin_macros` to August 2026 while ULMDB pins the macros to October
2026; Cargo rejects a combined dependency graph. Neither dependency was patched.
This is a concrete blocker to direct in-process adoption of this crate version,
even if its lookup measurements win. The benchmark does not qualify bbtree
integration or reproduce an atomic cross-process publication protocol.
