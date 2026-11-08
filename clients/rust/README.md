# ulmdb-rs

## Build

```sh
# From the repository root
cargo build -p ulmdb-rs --locked
```

```toml
# Your application's Cargo.toml
[dependencies]
ulmdb-rs = { path = "/path/to/ulmdb/clients/rust" }
```

## Use

```rust
use ulmdb_rs::{DatabaseKind, Env};

fn main() -> ulmdb_rs::Result<()> {
    let env = Env::create("example.ulmdb")?;
    env.update(|write| {
        write.create_database(b"items", DatabaseKind::Kv)?;
        write.put(b"items", b"key", b"value")
    })?;

    let read = env.read();
    assert_eq!(read.get(b"items", b"key")?, Some(b"value".as_slice()));
    Ok(())
}
```

- `Env::create`: path must be absent; parent directory must exist.
- `Env::open`: reopen an existing ULMDB environment.
- `update`: commits on `Ok`; aborts on error. Dropping an uncommitted writer aborts.
- Read values borrow their snapshot; missing keys return `None`.
- `Error::OutcomeUnknown`: reopen the environment before resolving the commit outcome.
- Examples: [ordered bytes](examples/basic.rs), [IPv4/IPv6](examples/routes.rs).
- [Native API](src/lib.rs) · [LMDB migration](../lmdb-ulmdb/README.md) · [Engine status](../../README.md)

## Test

```sh
# From the repository root
cargo test -p ulmdb-rs --locked
```
