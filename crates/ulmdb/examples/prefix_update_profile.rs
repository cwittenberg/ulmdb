//! Deterministic IPv4/IPv6 batch-upsert profile, separate from KV controls.
//! Run: prefix_update_profile NEW_DIRECTORY ROWS REPEAT legacy|indexed point|batch ipv4|ipv6
use std::{hint::black_box, path::PathBuf, time::Instant};
use ulmdb::{Address, DatabaseKind, Environment, Prefix, PublicationMode};

fn value(id: u64, version: u64) -> [u8; 64] {
    let mut bytes = [0; 64];
    bytes[..8].copy_from_slice(&id.to_le_bytes());
    bytes[8..16].copy_from_slice(&version.to_le_bytes());
    bytes
}
fn prefix(id: u64, v6: bool) -> Prefix {
    Prefix::new(
        if v6 {
            Address::V6(u128::from(id) << 64)
        } else {
            Address::V4(id as u32)
        },
        if v6 { 128 } else { 32 },
    )
    .unwrap()
}
fn main() -> Result<(), Box<dyn std::error::Error>> {
    let args: Vec<String> = std::env::args().collect();
    if args.len() != 7 {
        return Err("usage: prefix_update_profile NEW_DIRECTORY ROWS REPEAT legacy|indexed point|batch ipv4|ipv6".into());
    }
    let path = PathBuf::from(&args[1]);
    let rows: usize = args[2].parse()?;
    let repeat: u64 = args[3].parse()?;
    if !(256..=u32::MAX as usize).contains(&rows) {
        return Err("ROWS must be between 256 and u32::MAX".into());
    }
    let mode = match args[4].as_str() {
        "legacy" => PublicationMode::Legacy,
        "indexed" => PublicationMode::Indexed,
        _ => return Err("mode must be legacy or indexed".into()),
    };
    let batch = match args[5].as_str() {
        "point" => false,
        "batch" => true,
        _ => return Err("method must be point or batch".into()),
    };
    let v6 = match args[6].as_str() {
        "ipv4" => false,
        "ipv6" => true,
        _ => return Err("family must be ipv4 or ipv6".into()),
    };
    let env = Environment::create_with_publication(&path, mode)?;
    let mut write = env.begin_write()?;
    write.create_database(
        b"prefix",
        if v6 {
            DatabaseKind::Ipv6
        } else {
            DatabaseKind::Ipv4
        },
    )?;
    write.load_sorted_prefixes(
        b"prefix",
        (0..rows as u64).map(|id| Ok((prefix(id, v6), value(id, 0)))),
    )?;
    write.commit()?;
    println!("metric,repeat,operations,elapsed_ns,dirty_bytes");
    let old = env.begin_read();
    for (phase, (metric, count, stride)) in [
        ("prefix_sparse", (rows / 256).max(1), 256),
        ("prefix_dense25", rows / 4, 1),
        ("prefix_dense100", rows, 1),
    ]
    .into_iter()
    .enumerate()
    {
        let records: Vec<_> = (0..count as u64)
            .map(|offset| {
                let id = offset * stride;
                (prefix(id, v6), value(id, repeat * 16 + phase as u64 + 1))
            })
            .collect();
        let start = Instant::now();
        let mut write = env.begin_write()?;
        if batch {
            write.upsert_sorted_prefixes(b"prefix", records.iter().map(|(p, v)| Ok((*p, v))))?;
        } else {
            for (prefix, value) in &records {
                write.insert_prefix(b"prefix", *prefix, value)?;
            }
        }
        let mutation_elapsed = start.elapsed().as_nanos();
        let dirty = write.dirty_bytes();
        let commit = Instant::now();
        write.commit()?;
        let commit_elapsed = commit.elapsed().as_nanos();
        let total_elapsed = start.elapsed().as_nanos();
        println!("{metric}_mutation,{repeat},{count},{mutation_elapsed},{dirty}");
        println!("{metric}_commit,{repeat},{count},{commit_elapsed},{dirty}");
        println!("{metric}_total,{repeat},{count},{total_elapsed},{dirty}");
        let read = env.begin_read();
        for (prefix, expected) in &records {
            assert_eq!(read.exact_prefix(b"prefix", *prefix)?.unwrap().1, expected);
        }
    }
    assert_eq!(
        old.exact_prefix(b"prefix", prefix(0, v6))?.unwrap().1,
        value(0, 0)
    );
    drop(old);
    let read = env.begin_read();
    let start = Instant::now();
    let operations = 1_000_000u64;
    let mut state = 0x1b87_3593_c826_14a5u64;
    for _ in 0..operations {
        state ^= state << 13;
        state ^= state >> 7;
        state ^= state << 17;
        black_box(read.exact_prefix(b"prefix", prefix(state % rows as u64, v6))?);
    }
    println!(
        "prefix_lookup,{repeat},{operations},{},0",
        start.elapsed().as_nanos()
    );
    Ok(())
}
