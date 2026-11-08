# Allocator refusal and same-snapshot recovery gate

This Linux-only experiment checks the completed public hash preparation API when
the operating system refuses its address-space allocation. It does not measure
OS page reclamation, cgroup memory pressure, physical-memory exhaustion, swap,
page faults, throughput or latency percentiles.

The safe Rust child streams 2M sorted 8-byte keys/values into a Legacy database,
keeps its read transaction, verifies every native source record, and announces
its required hash allocation and valid admission budgets. No input fixture Vec
is retained. The parent then sets only its owned child's RLIMIT_AS soft limit
to current VmSize plus 8 MiB and permits hash preparation. The capability should
return `LimitExceeded`, while original native and Ordered readers still verify
all source bytes and preserve the exact commit identity.

The parent restores the same child's original limit and requests an identical
hash preparation retry in the same read transaction. The retry must succeed and
verify every hash value. This positive control prevents a configured quota,
unrelated error, source corruption or a poisoned transaction from masquerading
as successful allocator-failure handling.

Before admission the controller sums **all** private writable inode-zero process
mappings, including used memory and stacks. It requires the requested allocation
to exceed that conservative possible allocator-reuse upper bound plus remaining
headroom. If malloc nevertheless succeeds under the cap, or that bound is not
strong enough, the result is **inconclusive**, never a pass. An unexpected error,
panic, signal, identity change or failed recovery is a failure. Raw child output,
stderr, actual applied/restored limits, executable digest and verdict are retained.

Build/run after other performance measurements finish:

```sh
cargo build --release --locked --manifest-path benchmark/lookup-strategies/allocation/Cargo.toml
python3 -m unittest discover -s benchmark/lookup-strategies/allocation -p test_controller.py
python3 benchmark/lookup-strategies/allocation/controller.py \
  --binary benchmark/lookup-strategies/allocation/target/release/ulmdb-allocation-refusal-probe \
  --output benchmark/work/NEW-allocation-refusal
```

Controller exit codes: 0 pass, 1 failure, 2 inconclusive. The source database and
all evidence remain at the fresh output path. Parent/host resource limits are
never modified. The child contains no unsafe code and no custom allocator.
