//! Invoked by clients/tests/interoperability.py to seed/verify one shared database.
use ulmdb_rs::{DatabaseKind, Env, Error, Result, address, prefix};

const KV: &[u8] = b"interop-kv";
const V4: &[u8] = b"interop-v4";
const V6: &[u8] = b"interop-v6";

fn seed(path: &str) -> Result<()> {
    let env = Env::create(path)?;
    env.update(|write| {
        write.create_database(KV, DatabaseKind::Kv)?;
        write.create_database(V4, DatabaseKind::Ipv4)?;
        write.create_database(V6, DatabaseKind::Ipv6)?;
        write.put(KV, b"\0rust\xff", b"rust\0value\xff")?;
        write.insert_prefix(V4, prefix("192.0.2.0/24")?, b"rust-v4")?;
        write.insert_prefix(V6, prefix("2001:db8::/32")?, b"rust-v6")
    })?;
    println!("Rust seeded binary KV and IPv4/IPv6 indexes");
    Ok(())
}

fn verify(path: &str) -> Result<()> {
    let env = Env::open(path)?;
    let read = env.read();
    for (key, value) in [
        (b"\0rust\xff".as_slice(), b"rust\0value\xff".as_slice()),
        (b"\0python\xfe".as_slice(), b"python\0value\xfe".as_slice()),
        (b"\0node\xfd".as_slice(), b"node\0value\xfd".as_slice()),
    ] {
        assert_eq!(read.get(KV, key)?, Some(value));
    }
    for (db, ip, cidr, value) in [
        (V4, "192.0.2.10", "192.0.2.0/24", b"rust-v4".as_slice()),
        (V4, "192.0.2.130", "192.0.2.128/25", b"python-v4".as_slice()),
        (V4, "192.0.2.200", "192.0.2.192/26", b"node-v4".as_slice()),
        (V6, "2001:db8:2::1", "2001:db8::/32", b"rust-v6".as_slice()),
        (
            V6,
            "2001:db8:1:3::1",
            "2001:db8:1::/48",
            b"python-v6".as_slice(),
        ),
        (
            V6,
            "2001:db8:1:2::1",
            "2001:db8:1:2::/64",
            b"node-v6".as_slice(),
        ),
    ] {
        assert_eq!(
            read.longest_match(db, address(ip)?)?,
            Some((prefix(cidr)?, value)),
            "lookup {ip} after writes from all clients"
        );
    }
    println!("Rust verified records and nested prefixes written by Python and Node");
    Ok(())
}

fn main() -> Result<()> {
    let mut args = std::env::args().skip(1);
    let action = args
        .next()
        .ok_or(Error::InvalidInput("expected seed|verify PATH"))?;
    let path = args
        .next()
        .ok_or(Error::InvalidInput("expected seed|verify PATH"))?;
    if args.next().is_some() {
        return Err(Error::InvalidInput("expected seed|verify PATH"));
    }
    match action.as_str() {
        "seed" => seed(&path),
        "verify" => verify(&path),
        _ => Err(Error::InvalidInput("expected seed|verify PATH")),
    }
}
