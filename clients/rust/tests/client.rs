use std::{
    fs,
    ops::Bound,
    path::PathBuf,
    sync::atomic::{AtomicU64, Ordering},
};
use ulmdb_rs::{Address, DatabaseKind, Env, Error, Result, address, prefix};

struct Directory(PathBuf);

impl Directory {
    fn new() -> Self {
        static NEXT: AtomicU64 = AtomicU64::new(0);
        Self(std::env::temp_dir().join(format!(
            "ulmdb-rs-{}-{}-{}",
            std::process::id(),
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_nanos(),
            NEXT.fetch_add(1, Ordering::Relaxed)
        )))
    }
}

impl Drop for Directory {
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.0);
    }
}

#[test]
fn bytes_snapshot_and_reopen() -> Result<()> {
    let directory = Directory::new();
    let env = Env::open_or_create(&directory.0)?;
    assert_eq!(
        env.update(|write| {
            write.create_database(b"bytes", DatabaseKind::Kv)?;
            write.put(b"bytes", b"\0\xff", b"old\0\xff")?;
            write.put(b"bytes", b"empty", b"")?;
            Ok(42)
        })?,
        42
    );
    let before = env.read();
    let borrowed = before.get(b"bytes", b"\0\xff")?.unwrap();
    env.update(|write| write.put(b"bytes", b"\0\xff", b"new"))?;
    assert_eq!(borrowed, b"old\0\xff");
    assert_eq!(
        env.read().get(b"bytes", b"\0\xff")?,
        Some(b"new".as_slice())
    );
    drop(before);
    drop(env);
    let reopened = Env::open_or_create(&directory.0)?;
    let read = reopened.read();
    assert_eq!(read.get(b"bytes", b"\0\xff")?, Some(b"new".as_slice()));
    assert_eq!(read.get(b"bytes", b"empty")?, Some(b"".as_slice()));
    assert_eq!(read.get(b"bytes", b"missing")?, None);
    Ok(())
}

#[test]
fn closure_error_and_dropped_writer_abort() -> Result<()> {
    let directory = Directory::new();
    let env = Env::create(&directory.0)?;
    env.update(|write| write.create_database(b"kv", DatabaseKind::Kv))?;
    let result: Result<()> = env.update(|write| {
        write.put(b"kv", b"failed", b"uncommitted")?;
        Err(Error::InvalidInput("abort fixture"))
    });
    assert!(matches!(result, Err(Error::InvalidInput("abort fixture"))));
    {
        let mut write = env.write()?;
        write.put(b"kv", b"dropped", b"uncommitted")?;
    }
    assert_eq!(env.read().get(b"kv", b"failed")?, None);
    assert_eq!(env.read().get(b"kv", b"dropped")?, None);
    env.update(|write| write.put(b"kv", b"committed", b"yes"))?;
    assert_eq!(
        env.read().get(b"kv", b"committed")?,
        Some(b"yes".as_slice())
    );
    Ok(())
}

#[test]
fn ordered_scan_duplicates_and_savepoints() -> Result<()> {
    let directory = Directory::new();
    let env = Env::create(&directory.0)?;
    env.update(|write| {
        write.create_database(b"kv", DatabaseKind::Kv)?;
        write.create_database(b"duplicates", DatabaseKind::Duplicates { width: None })?;
        for key in [b"c", b"a", b"b"] {
            write.put(b"kv", key, key)?;
        }
        for value in [b"z", b"a", b"z"] {
            write.insert_duplicate(b"duplicates", b"key", value)?;
        }
        {
            let mut child = write.savepoint()?;
            child.put(b"kv", b"rollback", b"never visible")?;
        }
        Ok(())
    })?;
    let read = env.read();
    let keys = read
        .range(b"kv", Bound::Included(b"b"), Bound::Unbounded, true)?
        .map(|entry| entry.map(|(key, _)| key.to_vec()))
        .collect::<Result<Vec<_>>>()?;
    assert_eq!(keys, [b"c".to_vec(), b"b".to_vec()]);
    assert_eq!(read.get(b"kv", b"rollback")?, None);
    assert_eq!(read.duplicate_count(b"duplicates", b"key")?, 2);
    assert_eq!(
        read.duplicate_first(b"duplicates", b"key")?,
        Some(b"a".as_slice())
    );
    Ok(())
}

#[test]
fn ipv4_ipv6_atomic_commit_longest_match_and_delete() -> Result<()> {
    let directory = Directory::new();
    let env = Env::create(&directory.0)?;
    let p4 = prefix("192.0.2.0/24")?;
    let specific = prefix("192.0.2.128/25")?;
    let p6 = prefix("2001:db8::/32")?;
    env.update(|write| {
        write.create_database(b"ipv4", DatabaseKind::Ipv4)?;
        write.create_database(b"ipv6", DatabaseKind::Ipv6)?;
        write.insert_prefix(b"ipv4", p4, b"v4")?;
        write.insert_prefix(b"ipv4", specific, b"specific")?;
        write.insert_prefix(b"ipv6", p6, b"v6")
    })?;
    let old = env.read();
    assert_eq!(
        old.longest_match(b"ipv4", address("192.0.2.200")?)?,
        Some((specific, b"specific".as_slice()))
    );
    assert_eq!(
        old.longest_match(b"ipv6", address("2001:db8::42")?)?,
        Some((p6, b"v6".as_slice()))
    );
    assert!(matches!(
        old.longest_match(b"ipv4", address("::1")?),
        Err(Error::WrongKind)
    ));
    assert_eq!(old.longest_match(b"ipv4", address("198.51.100.1")?)?, None);
    env.update(|write| {
        write.delete_prefix(b"ipv4", specific)?;
        write.delete_prefix(b"ipv6", p6)
    })?;
    let new = env.read();
    assert_eq!(
        new.longest_match(b"ipv4", address("192.0.2.200")?)?,
        Some((p4, b"v4".as_slice()))
    );
    assert_eq!(new.longest_match(b"ipv6", address("2001:db8::42")?)?, None);
    assert_eq!(old.exact_prefix(b"ipv6", p6)?, Some((p6, b"v6".as_slice())));
    drop(new);
    drop(old);
    drop(env);
    let env = Env::open(&directory.0)?;
    assert_eq!(
        env.read().longest_match(b"ipv4", address("192.0.2.200")?)?,
        Some((p4, b"v4".as_slice()))
    );
    assert_eq!(
        env.read()
            .longest_match(b"ipv6", address("2001:db8::42")?)?,
        None
    );
    Ok(())
}

#[test]
fn text_parsing_is_strict() -> Result<()> {
    assert_eq!(address("192.0.2.1")?, Address::V4(0xc0000201));
    assert_eq!(address("::1")?, Address::V6(1));
    for text in [
        "192.0.2.1/24",
        "192.0.2.0/33",
        "::1/64",
        "::/129",
        "::/-1",
        "::/256",
        "::",
        "bad/4",
    ] {
        assert!(prefix(text).is_err(), "accepted {text}");
    }
    assert!(address("999.0.0.1").is_err());
    assert_eq!(prefix("0.0.0.0/0")?.length(), 0);
    assert_eq!(prefix("::/0")?.length(), 0);
    assert_eq!(prefix("2001:db8::/32")?.length(), 32);
    Ok(())
}

#[test]
fn open_or_create_preserves_invalid_existing_files() -> Result<()> {
    let directory = Directory::new();
    fs::create_dir(&directory.0)?;
    fs::write(directory.0.join("keep"), b"contents")?;
    assert!(Env::open_or_create(&directory.0).is_err());
    assert_eq!(fs::read(directory.0.join("keep"))?, b"contents");
    Ok(())
}
