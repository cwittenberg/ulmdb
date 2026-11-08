//! Run with: cargo run -p ulmdb-rs --example routes -- /tmp/new-ulmdb-routes
use ulmdb_rs::{DatabaseKind, Env, Error, Result, address, prefix};

fn main() -> Result<()> {
    let path = std::env::args()
        .nth(1)
        .ok_or(Error::InvalidInput("supply a new database path"))?;
    let env = Env::create(path)?;
    env.update(|write| {
        write.create_database(b"routes4", DatabaseKind::Ipv4)?;
        write.create_database(b"routes6", DatabaseKind::Ipv6)?;
        write.insert_prefix(b"routes4", prefix("192.0.2.0/24")?, b"example-v4")?;
        write.insert_prefix(b"routes4", prefix("192.0.2.128/25")?, b"specific-v4")?;
        write.insert_prefix(b"routes6", prefix("2001:db8::/32")?, b"example-v6")
    })?;

    let read = env.read();
    for (database, ip) in [
        (b"routes4".as_slice(), "192.0.2.200"),
        (b"routes6".as_slice(), "2001:db8::1"),
    ] {
        if let Some((network, value)) = read.longest_match(database, address(ip)?)? {
            println!("{ip} -> {network:?}: {}", String::from_utf8_lossy(value));
        }
    }
    Ok(())
}
