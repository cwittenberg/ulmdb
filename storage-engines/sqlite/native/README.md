# Private native SQLite backend

This standalone Rust static library supplies the small `usql_*` ABI used only
by the optional `ulmdb_owned` SQLite module. It is not an LMDB replacement. It
uses ULMDB's existing `snapshot-log` backend without changing ULMDB core, its
features' defaults, or the existing C compatibility library.

```sh
make -C storage-engines/sqlite owned
```

The bridge output is
`target/sqlite-native-owned-versioned/release/libulmdb_sqlite_native.a`;
the loadable module is `storage-engines/sqlite/ulmdb_sqlite_owned.so`.
See the [parent guide](../README.md) for SQLite loading and SQL syntax.

The owned Make target and container select the private `versioned` feature,
which uses the existing engine's versioned values and exact-key lookup strategy.
The standalone Cargo package has no default features. To build its static
library directly, select the same feature explicitly:

```sh
cargo build --release --locked --features versioned \
  --manifest-path storage-engines/sqlite/native/Cargo.toml \
  --target-dir target/sqlite-native-owned-versioned
```

This variant keeps the same total 64 MiB index budget and 64 MiB log bank
capacity. The native engine divides its index budget between ordered keys and
versioned values. For an ordered control, use a separate target directory and
`make ... owned ULMDB_NATIVE_FEATURES= ULMDB_NATIVE_TARGET=/absolute/target`.
This selection applies only to the optional SQLite owner; ULMDB core defaults
remain unchanged. Record size and SQL overhead still determine whether it wins.

The native engine's immutable index is authoritative. It reconstructs that
index from its synchronous journal on open. The bridge retains genuine native
read transactions while SQLite cursors use them; it does not cache results
between queries. One process owns the `SNAPSHOTLOG` file exclusively. Connections
inside that process share one checked native owner per canonical directory;
other processes receive BUSY until the final environment/transaction/cursor
owner closes. Namespace identity is checked when attaching another connection.
Inherited handles and inherited registry state are rejected after `fork` before
locks or filesystem changes. On Linux a private process-lifetime system page
uses `MADV_WIPEONFORK`: the kernel clears its token even for raw `SYS_fork`.
Each boundary checks that token; unsupported kernels fall back to checking the
actual PID. The guard uses atomic publication rather than an initialization
mutex. Open the database afresh after an exec.

No borrowed native writer crosses an FFI boundary. A SQL writer reserves the
owner and stages its actual write set in an ordered container. Monotonic writes
append to a sorted vector; a new out-of-order key or non-tail removal promotes
that transaction once to a BTreeMap. Small values up to 64 bytes stay inline;
larger values use owned buffers. Reads merge that delta
with its genuine base snapshot. Savepoints use a bounded undo journal; releasing
a child does not publish anything. The outer commit opens one native writer,
applies the final sorted delta and synchronously commits it once. Aborts release
the reservation and staged data. Other writers in the process receive BUSY while
the reservation is held. Read-only cursors iterate their retained native snapshot;
writer cursors seek and merge the base and delta within each callback, so no
borrowed writer cursor survives a mutation.

The private read-carrier API can retain one empty allocation between queries.
`usql_read_recycle` requires a live read with no cursors, drops its complete
snapshot, bound view and owner Arc, frees cursor-list storage, and leaves only
an Idle header. `usql_read_renew` installs a newly captured native head into that
header; it never revives an earlier snapshot. An Idle header owns no record,
pin, writer reservation or file lease and can move sequentially between threads.
Only `usql_read_discard` frees it. Active transaction handles stay thread-bound.
Closing the final environment releases the native owner even if an empty header
has not yet been discarded. This is allocation reuse, without query-data reuse.

Two SQL-shaped shortcuts avoid repeating generic byte-key marshaling.
`usql_read_point` captures a fresh head and looks up a signed integer key in one
checked boundary, leaving its real read active through result consumption.
`usql_cursor_row` advances or seeks an existing read cursor and returns a checked
signed key plus borrowed value. Both retain the same process, thread, schema and
value-size checks. The frontend still uses SQLite's copying result API; these
shortcuts do not retain results or reuse a snapshot between queries. Generic
operations remain available for writer scans and interface tests.

The private profile sets an authoritative index budget of 64 MiB, a 64 MiB log
bank capacity (two banks in one disk file), and a native transaction budget of
64 MiB minus 4096 bytes. The bridge admits at most 65,536 distinct changed keys
and 64 MiB of staged values plus conservative map/undo entry charges. Savepoint
metadata is separately bounded to 1024 nested scopes. Individual values are
limited to 16 MiB. Exceeding a budget fails explicitly; there is no unbounded
fallback. Checkpoints and retained readers also consume native index capacity.
The selected 32 MiB value pool supports at most 65,536 live key slots.
A nearly full journal may refuse a transaction that cannot fit beside the next
checkpoint. These budgets are fixed for this initial optional profile.

Persistent commits use the existing snapshot-log synchronous positioned-write
path, with a positioned-write plus `fdatasync` fallback. No asynchronous flag,
RAM drive, sidecar, or durability downgrade is used. The underlying engine's
power-loss qualification limits still apply. Its storage format is separate from
Legacy/Cohort/Indexed and Resident; the bridge rejects those environments.
Snapshot-log files must not be modified, restored, truncated or replaced while
an owner is live. SQLite catalog storage and the external engine still lack a
shared crash-atomic commit, as described in the parent guide.

Read-only transactions hold their genuine native snapshot directly, without a
write-set map, scope vector or shared mutable wrapper. Their once-bound read
view and cursor iterators borrow that snapshot's stable heap allocation. Every
cursor registers with the transaction; all close/abort/commit paths destroy
iterators before releasing the snapshot. Process/thread checks precede access
to that state. Borrowed get data remains valid until its transaction is mutated
or ended; cursor data until the next step, cursor close or transaction end.
The frontend copies values when required by SQLite's result lifetime.

Focused bridge checks:

```sh
cargo test --locked --features versioned \
  --manifest-path storage-engines/sqlite/native/Cargo.toml
```
