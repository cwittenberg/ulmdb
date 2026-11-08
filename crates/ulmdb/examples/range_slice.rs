//! Actual Environment syscall/mechanism probe, not latency qualification.
use std::{fs, path::PathBuf};
use ulmdb::{Address, DatabaseKind, Environment, Prefix, PublicationMode};
fn main() -> Result<(), Box<dyn std::error::Error>> {
    let path = PathBuf::from(
        std::env::args()
            .nth(1)
            .ok_or("fresh database path required")?,
    );
    let env = Environment::create_with_publication(&path, PublicationMode::Range)?;
    let p4 = Prefix::new(Address::V4(0xc0000200), 24).expect("canonical fixture");
    let p6 = Prefix::new(Address::V6(0x20010db8000000000000000000000000), 32)
        .expect("canonical fixture");
    let mut w = env.begin_write()?;
    w.create_database(b"kv", DatabaseKind::Kv)?;
    w.create_database(b"v4", DatabaseKind::Ipv4)?;
    w.create_database(b"v6", DatabaseKind::Ipv6)?;
    for n in 0..512u64 {
        w.put(b"kv", &n.to_be_bytes(), &n.to_le_bytes())?;
    }
    w.insert_prefix(b"v4", p4, b"old")?;
    w.insert_prefix(b"v6", p6, b"old")?;
    w.commit()?;
    let old = env.begin_read();
    let borrowed = old
        .get(b"kv", &7u64.to_be_bytes())?
        .ok_or("old key absent")?;
    let mut dirty = 0;
    eprintln!("range-steady-begin");
    for n in 2..=9u64 {
        let mut w = env.begin_write()?;
        w.put(b"kv", &7u64.to_be_bytes(), &n.to_le_bytes())?;
        w.insert_prefix(b"v4", p4, &n.to_le_bytes())?;
        w.insert_prefix(b"v6", p6, &n.to_le_bytes())?;
        dirty += w.dirty_bytes();
        assert_eq!(w.commit()?, n);
    }
    eprintln!("range-steady-end");
    assert_eq!(borrowed, 7u64.to_le_bytes());
    ulmdb::backup::verify(&env)?;
    drop(env);
    let reopened = Environment::open(&path)?;
    assert_eq!(reopened.stats().sequence, 9);
    let read = reopened.begin_read();
    assert_eq!(
        read.get(b"kv", &7u64.to_be_bytes())?,
        Some(9u64.to_le_bytes().as_slice())
    );
    assert_eq!(read.exact_prefix(b"v4", p4)?.unwrap().1, 9u64.to_le_bytes());
    assert_eq!(read.exact_prefix(b"v6", p6)?.unwrap().1, 9u64.to_le_bytes());
    assert_eq!(borrowed, 7u64.to_le_bytes());
    let stat = fs::metadata(path.join("RANGE.pack"))?;
    #[cfg(unix)]
    let allocated = {
        use std::os::unix::fs::MetadataExt;
        stat.blocks() * 512
    };
    #[cfg(not(unix))]
    let allocated = 0;
    #[cfg(unix)]
    let (sealed_data, frame_payload_bytes) = {
        use std::os::unix::fs::FileExt;
        let file = fs::File::open(path.join("RANGE.pack"))?;
        let mut at = 196608;
        let mut data = 0;
        let mut payloads = 0;
        while at < stat.len() {
            let mut frame = [0; 64];
            file.read_exact_at(&mut frame, at)?;
            let ordinal = u64::from_le_bytes(frame[40..48].try_into()?);
            let payload = u64::from_le_bytes(frame[48..56].try_into()?);
            let length = u64::from_le_bytes(frame[56..64].try_into()?);
            if ordinal >= 3 {
                let mut header = [0; 272];
                file.read_exact_at(&mut header, at + 4096)?;
                data += u64::from_le_bytes(header[256..264].try_into()?);
                payloads += payload;
            }
            at += length;
        }
        (data, payloads)
    };
    #[cfg(not(unix))]
    let (sealed_data, frame_payload_bytes) = (0, 0);
    println!(
        "{{\"schema\":1,\"qualified\":false,\"backend\":\"Range\",\"commits\":8,\"mutation_dirty_bytes_before_catalog\":{dirty},\"sealed_data_bytes\":{sealed_data},\"frame_payload_bytes\":{frame_payload_bytes},\"logical_bytes\":{},\"allocated_bytes\":{allocated}}}",
        stat.len()
    );
    Ok(())
}
