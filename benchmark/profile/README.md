# Profiling without perf permissions

This opt-in Linux x86_64 tool samples interrupted instruction pointers using
`ITIMER_PROF` and `SIGPROF`. It does not require `perf_event_open`. The sampler is
an `LD_PRELOAD` development library; it is never part of the database runtime.

```sh
python3 benchmark/profile/profile.py run \
  --output benchmark/raw/profile-write.csv --cpu 4 \
  -- /absolute/path/to/existing-release-binary its arguments
```

Choose an available CPU that is not running an accepted benchmark. Build the
target first, and invoke its executable directly. Do not target `cargo`, a shell,
the benchmark Python runner, or another process that forks/execs: only normal
target-process exit produces the sampled process's report. Target the already
built engine or millions benchmark executable with its normal arguments. Keep
profile runs separate from qualifying benchmark measurements.

The wrapper compiles the small C library using `cc`, runs the existing binary,
and writes three files: sample CSV, `.meta.json` mapping/provenance metadata, and
`.report.txt` symbolized hotspots. Output paths must be fresh. `addr2line` reads
existing debug information; release binaries should retain `debug = 1`. It
resolves the main ELF's load bias and loaded shared-library executable ranges.
The report includes shared and unresolved samples in its percentage denominator.
It also records DWARF inline expansion, which is not a runtime caller stack.

Defaults request one tick per 1,000 microseconds of process CPU time and reserve
1,000,000 samples (16 MB). `--interval-us` accepts 100..1,000,000;
`--max-samples` accepts 1..10,000,000. Linux timer granularity may make a requested
sub-millisecond interval ineffective. Once capacity is reached, later signals
are counted as dropped; earlier samples remain available. Signal coalescing,
blocked signals, and scheduling can cause additional uncounted misses. A short
run may deliver no samples. The timer consumes user and system CPU time, but RIP
attributes interrupted user code; it cannot identify kernel execution costs.

The signal handler uses lock-free atomics to reserve bounded slots, reads
`CLOCK_MONOTONIC`, and stores RIP/timestamp. It does not allocate, unwind, resolve
symbols, log, or perform file I/O. Output and ELF mapping discovery happen after
the timer is disabled. The tool refuses an existing SIGPROF handler or CPU timer.
It assumes a dynamically linked executable and ordinary process-lifetime preload;
static binaries, manual `dlclose`, forked children and abrupt exits are unsupported.
Shared libraries are mapped at finalization; a library unloaded before exit can
remain unresolved. Multithreaded delivery is safely bounded but samples are not
tagged by thread, and process CPU ticks need not be delivered to the thread whose
CPU use advanced the timer. Prefer a single-threaded target for attribution.

Sampling has timer/signal, timestamp, buffer-page and symbolization overhead;
its timings are **never accepted benchmark results**. There is no kernel stack,
runtime call stack, exact CPU-time accounting or automatic workload phase
inference. A sampled instruction hotspot is an observation, not proof of the
cause of end-to-end latency. Confirm proposed changes with an isolated experiment
and repeat the full uninstrumented benchmark suite afterward.

Re-symbolize a saved profile, optionally selecting an explicit time interval:

```sh
python3 benchmark/profile/profile.py report benchmark/raw/profile-write.csv \
  --start-ms 2000 --end-ms 5000 --report benchmark/raw/profile-window.txt
```

Time filters are relative to sampler initialization, not to an inferred benchmark
phase. Use independently observed phase timestamps if attributing a window to a
specific operation. `--executable` can relocate the same exact saved main binary;
using a different build invalidates symbolization.
