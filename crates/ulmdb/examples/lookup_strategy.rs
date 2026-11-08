//! cargo run --release -p ulmdb --example lookup_strategy -- NEW_DIRECTORY
use std::ops::Bound;
use ulmdb::{
    DatabaseKind, Environment,
    lookup::{HashLimits, Options},
};

fn main() -> Result<(), Box<dyn std::error::Error>> {
    let directory = std::env::args_os()
        .nth(1)
        .ok_or("usage: lookup_strategy NEW_DIRECTORY")?;
    let environment = Environment::create(directory)?;
    let mut write = environment.begin_write()?;
    write.create_database(b"hosts", DatabaseKind::Kv)?;
    write.put(b"hosts", b"example.net", b"192.0.2.1")?;
    write.put(b"hosts", b"www.example.net", b"192.0.2.2")?;
    write.commit()?;

    let read = environment.begin_read();
    let lookup = read.prepare_lookup(
        b"hosts",
        Options::Hash(HashLimits {
            max_entries: 100,
            max_index_bytes: 16 * 1024,
            max_value_bytes: 1024,
            max_total_value_bytes: 64 * 1024,
        }),
    )?;
    assert_eq!(lookup.get(b"example.net")?, Some(b"192.0.2.1".as_slice()));
    assert_eq!(lookup.get(b"absent.example.net")?, None);

    // Ordered operations retain the native tree and its range semantics.
    for row in lookup.cursor(Bound::Unbounded, Bound::Unbounded, false)? {
        let (key, value) = row?;
        println!(
            "{} = {}",
            String::from_utf8_lossy(key),
            String::from_utf8_lossy(value)
        );
    }
    let mut write = environment.begin_write()?;
    write.put(b"hosts", b"example.net", b"198.51.100.1")?;
    write.commit()?;
    // Each prepared strategy remains attached to its own immutable generation.
    assert_eq!(lookup.get(b"example.net")?, Some(b"192.0.2.1".as_slice()));
    let next = environment.begin_read();
    assert_eq!(
        next.get(b"hosts", b"example.net")?,
        Some(b"198.51.100.1".as_slice())
    );
    println!(
        "lookup: {:?}; identity: {:?}",
        lookup.stats(),
        lookup.identity()
    );
    // No background index or writer hook survives dropping these borrowed views.
    drop(lookup);
    drop(read);
    Ok(())
}
