#[path = "bin/common/residency.rs"]
mod residency_options;
use std::{net::IpAddr, ops::Bound, path::Path};
use ulmdb::{Address, DatabaseKind, Environment, Error, Prefix, Result};

fn prefix(text: &str) -> Result<Prefix> {
    let (address, length) = text
        .split_once('/')
        .ok_or(Error::InvalidInput("CIDR requires /length"))?;
    let address: IpAddr = address
        .parse()
        .map_err(|_| Error::InvalidInput("invalid address"))?;
    let length = length
        .parse()
        .map_err(|_| Error::InvalidInput("invalid prefix length"))?;
    Prefix::new(address.into(), length)
        .map_err(|_| Error::InvalidInput("invalid/noncanonical CIDR"))
}
fn address(text: &str) -> Result<Address> {
    text.parse::<IpAddr>()
        .map(Address::from)
        .map_err(|_| Error::InvalidInput("invalid IP address"))
}
fn show(value: Option<&[u8]>) {
    match value {
        Some(v) => println!("{}", String::from_utf8_lossy(v)),
        None => println!("not found"),
    }
}
fn show_prefix(value: Option<(Prefix, &[u8])>) {
    if let Some((p, v)) = value {
        let ip = match p.network() {
            Address::V4(n) => IpAddr::V4(n.into()),
            Address::V6(n) => IpAddr::V6(n.into()),
        };
        println!("{ip}/{}\t{}", p.length(), String::from_utf8_lossy(v));
    } else {
        println!("not found");
    }
}
fn demo(path: &Path, mode: ulmdb::PublicationMode, policy: ulmdb::ResidencyPolicy) -> Result<()> {
    let env = Environment::create_with_residency(path, mode, policy)?;
    let old = env.begin_read();
    let mut write = env.begin_write()?;
    write.create_database(b"hostnames", DatabaseKind::Kv)?;
    write.create_database(b"routing4", DatabaseKind::Ipv4)?;
    write.create_database(b"routing6", DatabaseKind::Ipv6)?;
    write.put(b"hostnames", b"example.test", b"192.0.2.130")?;
    write.insert_prefix(b"routing4", prefix("192.0.2.0/24")?, b"documentation IPv4")?;
    write.insert_prefix(
        b"routing4",
        prefix("192.0.2.128/25")?,
        b"more-specific IPv4",
    )?;
    write.insert_prefix(b"routing6", prefix("2001:db8::/32")?, b"documentation IPv6")?;
    println!("committed sequence {}", write.commit()?);
    let read = env.begin_read();
    show(read.get(b"hostnames", b"example.test")?);
    show_prefix(read.longest_match(b"routing4", address("192.0.2.130")?)?);
    show_prefix(read.longest_match(b"routing6", address("2001:db8::1")?)?);
    println!(
        "old snapshot sequence {}; current {}",
        old.sequence(),
        read.sequence()
    );
    Ok(())
}
fn execute_file_read(
    command: &str,
    source: &Path,
    read: &ulmdb::ReadTxn,
    args: &[String],
) -> Result<()> {
    let at = |index: usize| {
        args.get(index)
            .map(String::as_str)
            .ok_or(Error::InvalidInput("missing command argument"))
    };
    match command {
        "get" => show(read.get(at(2)?.as_bytes(), at(3)?.as_bytes())?),
        "stats" => println!("{:?}", read.stats()),
        "check" => println!(
            "{:?}",
            ulmdb::backup::verify_independent_snapshot(source, read)?
        ),
        "export" | "restore" => println!(
            "{:?}",
            ulmdb::backup::export_independent_snapshot(source, read, at(2)?)?
        ),
        "lpm" => show_prefix(read.longest_match(at(2)?.as_bytes(), address(at(3)?)?)?),
        "exact" => show_prefix(read.exact_prefix(at(2)?.as_bytes(), prefix(at(3)?)?)?),
        "cover" => show_prefix(read.covering_prefix(at(2)?.as_bytes(), prefix(at(3)?)?)?),
        "scan" => {
            for item in read
                .range(at(2)?.as_bytes(), Bound::Unbounded, Bound::Unbounded, false)?
                .take(20)
            {
                let (k, v) = item?;
                println!(
                    "{}\t{}",
                    String::from_utf8_lossy(k),
                    String::from_utf8_lossy(v)
                );
            }
        }
        "dup-count" => println!(
            "{}",
            read.duplicate_count(at(2)?.as_bytes(), at(3)?.as_bytes())?
        ),
        "dup-scan" => {
            for item in read.duplicate_values(
                at(2)?.as_bytes(),
                at(3)?.as_bytes(),
                Bound::Unbounded,
                Bound::Unbounded,
                false,
            )? {
                println!("{}", String::from_utf8_lossy(item?));
            }
        }
        _ => return Err(Error::InvalidInput("unknown file read command")),
    }
    Ok(())
}
fn execute() -> Result<()> {
    let arguments: Vec<_> = std::env::args().skip(1).collect();
    let (policy, args) = residency_options::parse(&arguments)?;
    let command=args.first().map(String::as_str).ok_or(Error::InvalidInput("commands: create, create-cohort, create-indexed, create-range-v5, create-range, demo, demo-cohort, demo-indexed, demo-range-v5, demo-range, upgrade-publication, upgrade-inventory, create-db, put, get, delete, prefix-put, prefix-delete, lpm, exact, cover, scan, dup-put, dup-delete, dup-delete-all, dup-count, dup-scan, export, restore, check, stats, residency, compact, prune, reclaim, retain, retained, release, get-retained, history-policy, history-status, maintenance-step"))?;
    let path = args
        .get(1)
        .ok_or(Error::InvalidInput("database directory required"))?;
    if matches!(
        command,
        "create" | "create-cohort" | "create-indexed" | "create-range-v5" | "create-range"
    ) {
        let mode = if command == "create-range" {
            ulmdb::PublicationMode::Range
        } else if command == "create-range-v5" {
            ulmdb::PublicationMode::RangeV5
        } else if command == "create-indexed" {
            ulmdb::PublicationMode::Indexed
        } else if command == "create-cohort" {
            ulmdb::PublicationMode::Cohort
        } else {
            ulmdb::PublicationMode::Legacy
        };
        Environment::create_with_residency(path, mode, policy)?;
        println!("created ULMDB {path}");
        return Ok(());
    }
    if matches!(
        command,
        "demo" | "demo-cohort" | "demo-indexed" | "demo-range-v5" | "demo-range"
    ) {
        let mode = if command == "demo-range" {
            ulmdb::PublicationMode::Range
        } else if command == "demo-range-v5" {
            ulmdb::PublicationMode::RangeV5
        } else if command == "demo-indexed" {
            ulmdb::PublicationMode::Indexed
        } else if command == "demo-cohort" {
            ulmdb::PublicationMode::Cohort
        } else {
            ulmdb::PublicationMode::Legacy
        };
        return demo(Path::new(path), mode, policy);
    }
    if command == "restore" && args.get(2).is_none() {
        return Err(Error::InvalidInput("restore target required"));
    }
    if matches!(
        command,
        "get"
            | "stats"
            | "check"
            | "export"
            | "restore"
            | "lpm"
            | "exact"
            | "cover"
            | "scan"
            | "dup-count"
            | "dup-scan"
    ) && !ulmdb_storage::range::is_selected(Path::new(path))?
    {
        let (_owner, read) =
            ulmdb::CoordinatedEnvironment::open_with_initial_read_and_residency(path, policy)?;
        return execute_file_read(command, Path::new(path), &read, &args);
    }
    if command == "restore" {
        let target = args
            .get(2)
            .ok_or(Error::InvalidInput("restore target required"))?;
        let source = Environment::open_with_residency(path, policy)?;
        println!("{:?}", ulmdb::backup::export(&source, target)?);
        return Ok(());
    }
    let env = Environment::open_with_residency(path, policy)?;
    let at = |index: usize| {
        args.get(index)
            .map(String::as_str)
            .ok_or(Error::InvalidInput("missing command argument"))
    };
    match command {
        "upgrade-publication" => println!("{:032x}", env.upgrade_publication()?),
        "upgrade-inventory" => println!("{:032x}", env.upgrade_inventory()?),
        "export" => println!("{:?}", ulmdb::backup::export(&env, at(2)?)?),
        "check" => println!("{:?}", ulmdb::backup::verify(&env)?),
        "stats" => println!("{:?}", env.stats()),
        "residency" => println!("{:?} {:?}", env.residency_report(), env.residency_status()),
        "compact" => println!("{:?}", env.compact()?),
        "reclaim" => println!("{:?}", env.reclaim()?),
        "prune" => println!("{:?}", env.prune_inventory()?),
        "history-status" => println!("{:?}", env.history_availability()?),
        #[cfg(any(target_os = "linux", target_os = "macos", windows))]
        "history-policy" => {
            let policy = match at(2)? {
                "all" => ulmdb::HistoryPolicy::KeepAll,
                value => ulmdb::HistoryPolicy::Retain {
                    keep_last_commits: value.parse().map_err(|_| {
                        Error::InvalidInput("history policy must be all or a positive commit count")
                    })?,
                    budget: Default::default(),
                },
            };
            println!("{:?}", env.set_history_policy(policy)?);
        }
        #[cfg(any(target_os = "linux", target_os = "macos", windows))]
        "maintenance-step" => println!("{:?}", env.step_history_maintenance()?),
        "retain" => println!("{:?}", env.retain_snapshot(at(2)?, &env.begin_read())?),
        "retained" => {
            for snapshot in env.retained_snapshots()? {
                println!("{snapshot:?}");
            }
        }
        "release" => env.release_snapshot(at(2)?)?,
        "get-retained" => show(
            env.open_retained(at(2)?)?
                .get(at(3)?.as_bytes(), at(4)?.as_bytes())?,
        ),
        "create-db" => {
            let kind = match at(3)? {
                "kv" => DatabaseKind::Kv,
                "ipv4" => DatabaseKind::Ipv4,
                "ipv6" => DatabaseKind::Ipv6,
                "duplicates" => DatabaseKind::Duplicates { width: None },
                text if text.starts_with("duplicates:") => DatabaseKind::Duplicates {
                    width: Some(
                        text[11..]
                            .parse()
                            .map_err(|_| Error::InvalidInput("invalid duplicate width"))?,
                    ),
                },
                _ => {
                    return Err(Error::InvalidInput(
                        "kind must be kv/ipv4/ipv6/duplicates[:width]",
                    ));
                }
            };
            let mut w = env.begin_write()?;
            w.create_database(at(2)?.as_bytes(), kind)?;
            println!("{}", w.commit()?);
        }
        "put" => {
            let mut w = env.begin_write()?;
            w.put(at(2)?.as_bytes(), at(3)?.as_bytes(), at(4)?.as_bytes())?;
            println!("{}", w.commit()?);
        }
        "delete" => {
            let mut w = env.begin_write()?;
            w.delete(at(2)?.as_bytes(), at(3)?.as_bytes())?;
            println!("{}", w.commit()?);
        }
        "get" => show(env.begin_read().get(at(2)?.as_bytes(), at(3)?.as_bytes())?),
        "prefix-put" => {
            let mut w = env.begin_write()?;
            w.insert_prefix(at(2)?.as_bytes(), prefix(at(3)?)?, at(4)?.as_bytes())?;
            println!("{}", w.commit()?);
        }
        "prefix-delete" => {
            let mut w = env.begin_write()?;
            w.delete_prefix(at(2)?.as_bytes(), prefix(at(3)?)?)?;
            println!("{}", w.commit()?);
        }
        "lpm" => show_prefix(
            env.begin_read()
                .longest_match(at(2)?.as_bytes(), address(at(3)?)?)?,
        ),
        "exact" => show_prefix(
            env.begin_read()
                .exact_prefix(at(2)?.as_bytes(), prefix(at(3)?)?)?,
        ),
        "cover" => show_prefix(
            env.begin_read()
                .covering_prefix(at(2)?.as_bytes(), prefix(at(3)?)?)?,
        ),
        "scan" => {
            let read = env.begin_read();
            for item in read
                .range(at(2)?.as_bytes(), Bound::Unbounded, Bound::Unbounded, false)?
                .take(20)
            {
                let (k, v) = item?;
                println!(
                    "{}\t{}",
                    String::from_utf8_lossy(k),
                    String::from_utf8_lossy(v)
                );
            }
        }
        "dup-put" => {
            let mut write = env.begin_write()?;
            let inserted =
                write.insert_duplicate(at(2)?.as_bytes(), at(3)?.as_bytes(), at(4)?.as_bytes())?;
            println!("sequence {} inserted {inserted}", write.commit()?);
        }
        "dup-delete" => {
            let mut write = env.begin_write()?;
            let removed =
                write.delete_duplicate(at(2)?.as_bytes(), at(3)?.as_bytes(), at(4)?.as_bytes())?;
            println!("sequence {} removed {removed}", write.commit()?);
        }
        "dup-delete-all" => {
            let mut write = env.begin_write()?;
            let removed = write.delete_duplicates(at(2)?.as_bytes(), at(3)?.as_bytes())?;
            println!("sequence {} removed {removed}", write.commit()?);
        }
        "dup-count" => println!(
            "{}",
            env.begin_read()
                .duplicate_count(at(2)?.as_bytes(), at(3)?.as_bytes())?
        ),
        "dup-scan" => {
            let read = env.begin_read();
            for item in read.duplicate_values(
                at(2)?.as_bytes(),
                at(3)?.as_bytes(),
                Bound::Unbounded,
                Bound::Unbounded,
                false,
            )? {
                println!("{}", String::from_utf8_lossy(item?));
            }
        }
        _ => return Err(Error::InvalidInput("unknown command")),
    }
    Ok(())
}
fn main() {
    if let Err(e) = execute() {
        eprintln!("{e}");
        std::process::exit(1);
    }
}
