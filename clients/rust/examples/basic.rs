//! Run with: cargo run -p ulmdb-rs --example basic -- /tmp/new-ulmdb-rs
use std::ops::Bound;
use ulmdb_rs::{DatabaseKind, Env, Error, Result};

fn main() -> Result<()> {
    let path = std::env::args()
        .nth(1)
        .ok_or(Error::InvalidInput("supply a new database path"))?;
    let env = Env::create(path)?;
    env.update(|write| {
        write.create_database(b"users", DatabaseKind::Kv)?;
        write.put(b"users", b"alice", b"Alice")?;
        write.put(b"users", b"bob", b"Bob")
    })?;

    let read = env.read();
    assert_eq!(read.get(b"users", b"alice")?, Some(b"Alice".as_slice()));
    for row in read.range(b"users", Bound::Unbounded, Bound::Unbounded, false)? {
        let (key, value) = row?;
        println!(
            "{} = {}",
            String::from_utf8_lossy(key),
            String::from_utf8_lossy(value)
        );
    }
    env.update(|write| write.delete(b"users", b"bob"))?;
    // This reader continues to see its original snapshot.
    assert!(read.get(b"users", b"bob")?.is_some());
    assert!(env.read().get(b"users", b"bob")?.is_none());
    Ok(())
}
