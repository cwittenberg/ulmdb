# ulmdb-rs

The safe Rust client uses ULMDB directly. It preserves borrowed snapshot reads,
checked handles and the engine's complete transaction and IP index APIs.

Add this repository package as a path dependency:

```toml
[dependencies]
ulmdb-rs = { path = "/path/to/ulmdb/clients/rust" }
```

```rust
use ulmdb_rs::{DatabaseKind, Env};

fn main() -> ulmdb_rs::Result<()> {
    let env = Env::open_or_create("new-database.ulmdb")?;
    env.update(|write| {
        write.create_database(b"items", DatabaseKind::Kv)?;
        write.put(b"items", b"key", b"value")
    })?;
    let read = env.read();
    assert_eq!(read.get(b"items", b"key")?, Some(b"value".as_slice()));
    Ok(())
}
```

The first run creates `items`; on subsequent runs, open an existing database with
`read.open_database(b"items")` or `write.open_database(b"items")` instead of
calling `create_database` again. `Env::open_or_create` creates only absent paths;
its parent must exist. Existing paths must contain a valid ULMDB environment.

`Env::read()` pins a snapshot, `Env::write()` returns an explicit writer, and
`Env::update()` commits a successful closure or aborts when it returns an error.
Dropping a writer also aborts it. A commit can return `Error::OutcomeUnknown`;
follow the engine's requirement to reopen before resolving that result.

Keys and values are bytes. Missing reads return `None`, while present empty
values return `Some(&[])`. A read value borrows its transaction and remains valid
across later commits. `range` supports ordered bounds and reverse iteration;
`DatabaseKind::Duplicates` supplies sorted multiple values per key.

For IP indexes, `address("2001:db8::1")` and `prefix("2001:db8::/32")` parse
text into native `Address` and `Prefix` values. `prefix` rejects nonzero host
bits and invalid lengths. Use `Prefix::normalize` to normalize explicitly.
`insert_prefix`, `longest_match`, `exact_prefix`, `covering_prefix`, subnet
cursors and prefix deletion remain available through the reexported engine API.
Transactions can atomically update KV, IPv4, IPv6 and duplicate databases.

All engine public types and modules are reexported. Use `Environment` directly
for its existing constructors, or `Env::from_environment` to wrap a custom
publication/residency configuration. These clients use ULMDB's file format;
existing LMDB files require migration.

To migrate, keep the LMDB environment intact and create a separate ULMDB path.
Read one consistent LMDB snapshot, enumerate its databases with their flags, and
replay the raw key/value pairs into matching ULMDB databases. Use `put` for
ordinary KV databases and `insert_duplicate` for sorted duplicate databases;
choose `DatabaseKind::Duplicates { width: Some(n) }` for fixed-width values.
Verify counts and raw pairs, then change the application's database path. Custom
LMDB comparators and integer/reverse ordering require an explicit encoding or
application migration rather than assuming the same ordering. LMDB KV-encoded
IP data must be decoded and inserted with `insert_prefix` to use native IP
indexes. The Rust client exposes the ULMDB API; moving from an unrelated Rust
LMDB crate also requires adapting its transaction calls.

Run from the repository root:

```sh
cargo test -p ulmdb-rs
cargo run -p ulmdb-rs --example basic -- /tmp/new-ulmdb-rs
cargo run -p ulmdb-rs --example routes -- /tmp/new-ulmdb-routes
```

Example destination paths must be absent. Tests cover binary and empty values,
snapshot isolation, aborts, persisted reopen, ordering, duplicates, savepoints,
IPv4/IPv6 longest match, family errors and strict CIDR parsing.
