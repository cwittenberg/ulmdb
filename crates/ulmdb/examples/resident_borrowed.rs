//! Run the experimental borrowed-redo engine on a fresh disk or RAM-drive path.
use std::path::PathBuf;
use ulmdb::{
    Address, DatabaseKind, Prefix,
    resident::{Environment, Options, RedoEncoding, RedoFormat},
};

fn main() -> Result<(), Box<dyn std::error::Error>> {
    let path = PathBuf::from(
        std::env::args()
            .nth(1)
            .ok_or("fresh database path required")?,
    );
    let options = Options::default();
    let environment = Environment::create_with_encoding(
        &path,
        options,
        RedoFormat::CompactV2,
        RedoEncoding::Borrowed,
    )?;
    let v4 = Prefix::new(Address::V4(0xc000_0200), 24).expect("canonical IPv4 fixture");
    let v6 = Prefix::new(Address::V6(0x2001_0db8u128 << 96), 32).expect("canonical IPv6 fixture");
    let key = 7u64.to_be_bytes();
    let first = 1u64.to_be_bytes();
    let second = 2u64.to_be_bytes();

    let mut write = environment.begin_write()?;
    write.create_database(b"kv", DatabaseKind::Kv)?;
    write.create_database(b"v4", DatabaseKind::Ipv4)?;
    write.create_database(b"v6", DatabaseKind::Ipv6)?;
    write.put(b"kv", &key, &first)?;
    write.insert_prefix(b"v4", v4, &first)?;
    write.insert_prefix(b"v6", v6, &first)?;
    assert_eq!(write.commit()?, 1);

    let old = environment.begin_read();
    let borrowed = old.get(b"kv", &key)?.ok_or("missing first value")?;
    let old_v4 = old.prefix_index(b"v4")?;
    let old_v6 = old.prefix_index(b"v6")?;
    let mut write = environment.begin_write()?;
    write.put(b"kv", &key, &second)?;
    write.insert_prefix(b"v4", v4, &second)?;
    write.insert_prefix(b"v6", v6, &second)?;
    assert_eq!(write.commit()?, 2);
    assert_eq!(borrowed, first);
    assert_eq!(
        old_v4.longest_match(Address::V4(0xc000_0201))?.unwrap().1,
        first
    );
    assert_eq!(
        old_v6
            .longest_match(Address::V6((0x2001_0db8u128 << 96) | 1))?
            .unwrap()
            .1,
        first
    );

    drop(environment);
    let reopened = Environment::open_with_encoding(
        &path,
        options,
        RedoFormat::CompactV2,
        RedoEncoding::Borrowed,
    )?;
    let current = reopened.begin_read();
    assert_eq!(current.sequence(), 2);
    assert_eq!(current.get(b"kv", &key)?, Some(second.as_slice()));
    assert_eq!(current.prefix_index(b"v4")?.exact(v4)?.unwrap().1, second);
    assert_eq!(current.prefix_index(b"v6")?.exact(v6)?.unwrap().1, second);
    assert_eq!(borrowed, first);
    println!("Borrowed redo: atomic KV/IPv4/IPv6 updates, old snapshot and cold reopen passed.");
    Ok(())
}
