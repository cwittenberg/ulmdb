# Prefix lookup strategy experiment

This standalone, opt-in benchmark compares the existing checked binary radix
reader with the existing experimental checked four-bit compressed radix reader.
It changes no production source, feature default, file format, transaction path,
or algorithm selection. `Algorithm` is the experiment's explicit Strategy seam.

```sh
cargo test --manifest-path benchmark/lookup-strategies/prefix/Cargo.toml --locked
cargo build --release --manifest-path benchmark/lookup-strategies/prefix/Cargo.toml --locked
# Run only while other compilation/tests/benchmarks are idle. Select an allowed CPU.
taskset -c 4 benchmark/lookup-strategies/prefix/target/release/ulmdb-prefix-strategy-benchmark \
  --keys 100000 --queries 8192 --rounds 7 --iterations 4 --seed 1 > prefix.csv
```

The default enables the scalar-family binary reader used by the Resident
qualification. `--no-default-features` selects the generic binary reader instead.
Use separate binaries/output paths if comparing these two feature profiles.
Keys, query count, rounds, repetitions and seed are configurable as shown.
IPv4 and IPv6 run separately within each process, with 16-byte payloads.

Both structures use the same checked contiguous benchmark arena, frozen before
reads. Results isolate existing index traversal and decoder cost in memory;
they do not measure memory-mapped file opening, page faults, transactions,
durability, publication, garbage collection, or IPDon policy evaluation.

Three distributions run for exact, longest-prefix, and all-covering lookup:

- `hit`: shuffled uniform selection among inserted prefixes (100% hits).
- `mixed50`: 50% inserted prefixes, 25% upper-half misses outside the root,
  25% misses in a deliberately empty interior nibble-four region.
- `nested_chain`: one repeated address with at least seven IPv4 or fifteen IPv6
  ancestor claims. This intentionally hot case exercises multiple-candidate work.

All-covering means enumerating the complete `ancestors` cursor, longest-first,
with full borrowed payload consumption. It does not substitute the single-result
`covering` method. Both prepared point readers bind their root once outside the
timing region. Both cursor paths retain their public checked construction cost.

Every benchmark query first compares both algorithms' exact matched prefix,
complete payload bytes and full ordered ancestor vector. Deterministic probes
also compare the actual fixture to a linear independent oracle; a separate
128-key fixture validates both algorithms against that oracle. Timed loops
consume prefix plus both payload words, use `black_box`, and assert equal
checksums per sample. Each operation is warmed separately and strategy order
alternates each round. Orchestration should additionally use fresh processes.

CSV columns are:
`kind,family,keys,algorithm,operation,distribution,round,ns_per_op,checksum,build_ms,arena_bytes,reachable_bytes`.

`build` records report construction using each existing available path:
production sorted streaming bulk build for binary, incremental sorted inserts for
multibit (no multibit bulk builder currently exists). The difference is material
for snapshot construction and is explicitly not an identical insertion-algorithm
microbenchmark. `arena_bytes` includes alignment and abandoned private
allocations. `reachable_bytes` counts the union of bytes actually read by a full
checked scan of the final index, including payloads, excluding alignment and
unreachable build debris. It is not whole-process RSS or allocator capacity.
`lookup` records report individual warm timing samples; zero fields are unused.

This experiment supplies evidence to decide whether an additional optional
strategy deserves integration. Any production admission still needs its own
snapshot/publication lifecycle, mutation/reclamation costs, end-to-end IPDon
policy correctness and performance qualification.
