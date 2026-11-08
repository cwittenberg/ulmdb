//! Create a new, explicitly requested demo environment for exploring the UI.
use std::{net::IpAddr, path::PathBuf};
use ulmdb::{DatabaseKind, Environment, Prefix};

fn main() -> Result<(), Box<dyn std::error::Error>> {
    let path = PathBuf::from(
        std::env::args_os()
            .nth(1)
            .ok_or("usage: cargo run -p uexplorer --example demo -- NEW_DIRECTORY")?,
    );
    let env = Environment::create(&path)?;
    let mut write = env.begin_write()?;
    write.create_database(b"hostnames", DatabaseKind::Kv)?;
    write.create_database(b"observations", DatabaseKind::Duplicates { width: None })?;
    write.create_database(b"routing4", DatabaseKind::Ipv4)?;
    write.create_database(b"routing6", DatabaseKind::Ipv6)?;
    for number in 0..600 {
        let key = format!("host-{number:04}.example.test");
        let value = format!(
            "192.0.2.{} | demo record {number} | 🌱 documentation data",
            number % 254 + 1
        );
        write.put(b"hostnames", key.as_bytes(), value.as_bytes())?;
    }
    write.put(b"hostnames", b"binary-example", b"\x00\x01\x02\xff\x1b[2J")?;
    write.put(b"hostnames", b"welcome", "Welcome to ulmdb explorer! 🔎\nUse Tab to switch panes, / to search, and x to view hex.\nThis is an illustrative demo database.".as_bytes())?;
    for source in [
        b"collector-amsterdam".as_slice(),
        b"collector-tokyo",
        b"collector-new-york",
    ] {
        write.insert_duplicate(b"observations", b"example.test", source)?;
    }
    for (database, address, length, value) in [
        (
            b"routing4".as_slice(),
            "192.0.2.0",
            24,
            "Documentation IPv4",
        ),
        (b"routing4", "192.0.2.128", 25, "More specific route"),
        (b"routing6", "2001:db8::", 32, "Documentation IPv6"),
    ] {
        let prefix = Prefix::new(address.parse::<IpAddr>()?.into(), length)
            .map_err(|_| "invalid demo prefix")?;
        write.insert_prefix(database, prefix, value.as_bytes())?;
    }
    write.commit()?;
    println!("Demo created. Open it with:\nuexplorer {}", path.display());
    Ok(())
}
