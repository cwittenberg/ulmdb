# Resident/LMDB diagnostic probe

This standalone development crate measures transaction begin, mutation and commit
separately through the public Resident and pinned native LMDB adapters. It changes
no engine, accepted benchmark, durability setting or production dependency.
The nine Resident features match `benchmark/engine` and `benchmark/millions`.
Its additional timers perturb latency: results are **diagnostic**, never accepted
full-suite or million-key qualification results.

Build from the repository root:

```sh
CARGO_TARGET_DIR=benchmark/target/profile-probe cargo build --release --locked --manifest-path benchmark/profile/probe/Cargo.toml -j 1
```

Run serially on new directories on an actual tmpfs for RAM-write profiling:

```sh
benchmark/target/profile-probe/release/ulmdb-profile-probe --mode smallwrites --engine ulmdb --dir /dev/shm/ulmdb-probe-smallwrites
benchmark/target/profile-probe/release/ulmdb-profile-probe --mode smallwrites --engine lmdb --dir /dev/shm/lmdb-probe-smallwrites
benchmark/target/profile-probe/release/ulmdb-profile-probe --mode load --engine ulmdb --dir /dev/shm/ulmdb-probe-load
benchmark/target/profile-probe/release/ulmdb-profile-probe --mode load --engine lmdb --dir /dev/shm/lmdb-probe-load
```

`smallwrites` defaults to 8,192 keys, 20,000 measured transactions and one update
per transaction. The initial fixture load uses batches of 1,000 outside the
measurement. `load` defaults to 1,000,000 sorted keys and batches of 1,000;
its transaction count must equal `ceil(keys/batch)`. Both modes use the same
deterministic eight-byte keys/values and measured-trace hash across engines.
Use `--keys`, `--transactions` and `--batch` to run a smaller control. These
workloads diagnose the same mechanisms as the accepted suites but are independent
traces, without the full suite's backend-order alternation or statistical design.

JSON phase markers on stderr include PID, elapsed nanoseconds and absolute
`CLOCK_MONOTONIC` nanoseconds, matching the standalone PC sampler's clock.
`measurement_begin` and `measurement_end` bound the measured phase; fixture
generation/load, setup, validation and cold replay have separate markers. The
`reopen_complete` marker precedes post-open data validation, so replay/open cost
can be selected independently of that verification. The
optional `--profile-delay-ms 5000` pauses after `ready_for_profile`, before
`measurement_begin`, to permit profiler attachment. A profiler covering the whole
process will also observe setup and cold replay; filter by the measurement
markers before attributing samples or syscall costs to writes.

Stdout is one JSON object containing phase totals, sampled transaction and commit
latency, up to 2,048 transaction samples, input identity, configuration and
validation. Every run validates every expected key/value, every interleaved miss,
the full ordered cursor, and the same results after full close/reopen. Resident
also checks a borrowed old snapshot across one extra update outside measurement,
then restores the expected value. LMDB uses normal flags `0`; its thread-affine
adapter disallows a retained reader alongside a writer, so that extra snapshot
check is Resident-only and explicitly reported.

Resident uses 1,024 KV/1,024 Tiny pages at <=8,192 keys, or 8,192 KV/0 Tiny pages
above that threshold. The WAL is bounded at 256 MiB. Automatic Collect triggers
below one quarter of KV capacity and is included in transaction begin timing;
the existing prefix audit cache is enabled. Full WAL replay and cold graph audits
remain enabled. These bounds can reject long or large custom workloads; an error
does not produce a successful JSON report, and fixture files remain for diagnosis.
All directories must be new. Never run diagnostic profiles concurrently with
qualification benchmarks or use traced timings as performance evidence.
