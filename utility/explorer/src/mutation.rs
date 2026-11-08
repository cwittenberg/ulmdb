//! One-record transactions, fenced against the exact snapshot shown to the user.
use std::{net::IpAddr, path::Path};
use ulmdb::{CoordinatedEnvironment, DatabaseKind, Error, Prefix, ReadTxn};

pub const MAX_EDIT_BYTES: usize = 65_536;

#[derive(Clone, Debug)]
pub struct Target {
    pub database: Vec<u8>,
    pub kind: DatabaseKind,
    pub key: Vec<u8>,
    pub old_value: Vec<u8>,
    pub commit: u128,
    pub(crate) uuid: [u8; 16],
}

#[derive(Debug)]
pub enum Action {
    Replace(Vec<u8>),
    Delete,
}

pub(crate) fn prefix(key: &[u8]) -> ulmdb::Result<Prefix> {
    let text = std::str::from_utf8(key)
        .map_err(|_| Error::InvalidInput("selected CIDR is not valid text; refresh first"))?;
    let (address, length) = text
        .split_once('/')
        .ok_or(Error::InvalidInput("selected CIDR needs a prefix length"))?;
    Prefix::new(
        address
            .parse::<IpAddr>()
            .map_err(|_| Error::InvalidInput("selected CIDR address is invalid"))?
            .into(),
        length
            .parse()
            .map_err(|_| Error::InvalidInput("selected CIDR length is invalid"))?,
    )
    .map_err(|_| Error::InvalidInput("selected CIDR is noncanonical or invalid"))
}

pub(crate) fn value<'a>(
    read: &'a ReadTxn,
    kind: DatabaseKind,
    database: &[u8],
    key: &[u8],
    duplicate: Option<&[u8]>,
) -> ulmdb::Result<Option<&'a [u8]>> {
    match kind {
        DatabaseKind::Kv => read.get(database, key),
        DatabaseKind::Duplicates { .. } => {
            let expected = duplicate.ok_or(Error::InvalidInput(
                "selected duplicate identity is missing; refresh first",
            ))?;
            Ok(read
                .duplicate_seek_pair(database, key, expected)?
                .filter(|stored| *stored == expected))
        }
        DatabaseKind::Ipv4 | DatabaseKind::Ipv6 => Ok(read
            .exact_prefix(database, prefix(key)?)?
            .map(|(_, bytes)| bytes)),
    }
}

/// Hold a cooperating writer lease only for this transaction. The base commit
/// check occurs inside `update`, after acquiring writer exclusion, so a newer
/// commit cannot slip between checking the old value and selecting a mutation.
pub fn apply(path: &Path, target: &Target, action: Action) -> Result<u64, String> {
    if target.old_value.len() > MAX_EDIT_BYTES {
        return Err("This record is larger than the 64 KiB mutation limit; use the owning application to change it.".into());
    }
    if let Action::Replace(bytes) = &action {
        if bytes.len() > MAX_EDIT_BYTES {
            return Err("Edited values are limited to 64 KiB.".into());
        }
        if let DatabaseKind::Duplicates { width: Some(width) } = target.kind
            && bytes.len() != width as usize
        {
            return Err(format!(
                "This duplicate database requires exactly {width} bytes per value."
            ));
        }
    }
    let environment = CoordinatedEnvironment::open(path).map_err(write_error)?;
    environment
        .update(|write| {
            let read = write.transaction_start_snapshot()?;
            if read.commit_identity() != target.commit || read.file_manifest()?.uuid != target.uuid
            {
                return Err(Error::InvalidInput(
                    "the database changed since this record was selected; refresh and try again",
                ));
            }
            if read.database_kind(&target.database)? != target.kind {
                return Err(Error::InvalidInput(
                    "the selected database changed; refresh and try again",
                ));
            }
            let current = value(
                &read,
                target.kind,
                &target.database,
                &target.key,
                Some(&target.old_value),
            )?;
            if current != Some(target.old_value.as_slice()) {
                return Err(Error::InvalidInput(
                    "the selected record changed or disappeared; refresh and try again",
                ));
            }
            if matches!(&action, Action::Replace(bytes) if *bytes == target.old_value) {
                return Ok(read.sequence());
            }
            let sequence = read.sequence().checked_add(1).ok_or(Error::LimitExceeded)?;
            match (&action, target.kind) {
                (Action::Replace(bytes), DatabaseKind::Kv) => {
                    write.put(&target.database, &target.key, bytes)?
                }
                (Action::Delete, DatabaseKind::Kv) => {
                    if !write.delete_with_presence(&target.database, &target.key)? {
                        return Err(Error::InvalidInput(
                            "the selected key disappeared; refresh first",
                        ));
                    }
                }
                (Action::Replace(bytes), DatabaseKind::Duplicates { .. }) => {
                    if read.duplicate_seek_pair(&target.database, &target.key, bytes)?
                        == Some(bytes.as_slice())
                    {
                        return Err(Error::InvalidInput(
                            "that duplicate value already exists; the original record was kept",
                        ));
                    }
                    if !write.delete_duplicate(&target.database, &target.key, &target.old_value)? {
                        return Err(Error::InvalidInput(
                            "the selected duplicate disappeared; refresh first",
                        ));
                    }
                    if !write.insert_duplicate(&target.database, &target.key, bytes)? {
                        return Err(Error::InvalidInput(
                            "the replacement duplicate already exists; transaction cancelled",
                        ));
                    }
                }
                (Action::Delete, DatabaseKind::Duplicates { .. }) => {
                    if !write.delete_duplicate(&target.database, &target.key, &target.old_value)? {
                        return Err(Error::InvalidInput(
                            "the selected duplicate disappeared; refresh first",
                        ));
                    }
                }
                (Action::Replace(bytes), DatabaseKind::Ipv4 | DatabaseKind::Ipv6) => {
                    write.insert_prefix(&target.database, prefix(&target.key)?, bytes)?;
                }
                (Action::Delete, DatabaseKind::Ipv4 | DatabaseKind::Ipv6) => {
                    write.delete_prefix(&target.database, prefix(&target.key)?)?;
                }
            }
            Ok(sequence)
        })
        .map_err(write_error)
}

fn write_error(error: Error) -> String {
    match error {
        Error::Busy => "Another writer is using this database. Finish its transaction, then retry.".into(),
        Error::OutcomeUnknown => "The commit outcome is unknown. Refresh the database to inspect its state before retrying.".into(),
        other => format!("Could not change the record: {other}"),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::data::{Reader, Record, VALUE_PREVIEW_LIMIT};
    use std::{
        path::PathBuf,
        sync::atomic::{AtomicU64, Ordering},
    };
    use ulmdb::Environment;

    static NEXT: AtomicU64 = AtomicU64::new(0);
    struct Directory(PathBuf);
    impl Directory {
        fn new() -> Self {
            Self(std::env::temp_dir().join(format!(
                    "uexplorer-mutations-{}-{}-{}",
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
            let _ = std::fs::remove_dir_all(&self.0);
        }
    }
    fn rows(reader: &Reader, database: &[u8]) -> Vec<Record> {
        let page = reader.page(database, None, "", 256).unwrap();
        assert!(page.complete);
        page.records
    }
    fn selected(reader: &Reader, database: &[u8], key: &[u8], value: Option<&[u8]>) -> Target {
        let row = rows(reader, database)
            .into_iter()
            .find(|row| row.key == key && value.is_none_or(|value| row.value == value))
            .unwrap();
        reader.target(database, &row).unwrap()
    }
    fn contents(path: &Path, database: &[u8]) -> Vec<(Vec<u8>, Vec<u8>)> {
        rows(&Reader::open(path).unwrap(), database)
            .into_iter()
            .map(|row| (row.key, row.value))
            .collect()
    }

    #[test]
    fn edits_and_deletes_persist_for_all_kinds_without_removing_neighbor_records() {
        let directory = Directory::new();
        let env = Environment::create(&directory.0).unwrap();
        let mut write = env.begin_write().unwrap();
        for (name, kind) in [
            (b"kv".as_slice(), DatabaseKind::Kv),
            (b"dup", DatabaseKind::Duplicates { width: None }),
            (b"v4", DatabaseKind::Ipv4),
            (b"v6", DatabaseKind::Ipv6),
        ] {
            write.create_database(name, kind).unwrap();
        }
        write.put(b"kv", b"alice", b"Alice").unwrap();
        write.put(b"kv", b"bob", b"Bob").unwrap();
        for value in [b"a", b"b", b"c"] {
            write.insert_duplicate(b"dup", b"k", value).unwrap();
        }
        for (database, cidr, value) in [
            (
                b"v4".as_slice(),
                b"192.0.0.0/16".as_slice(),
                b"cover".as_slice(),
            ),
            (b"v4", b"192.0.2.0/24", b"target"),
            (b"v4", b"192.0.2.128/25", b"child"),
            (b"v6", b"2001:db8::/32", b"cover"),
            (b"v6", b"2001:db8:1::/48", b"target"),
            (b"v6", b"2001:db8:1:1::/64", b"child"),
        ] {
            write
                .insert_prefix(database, prefix(cidr).unwrap(), value)
                .unwrap();
        }
        write.commit().unwrap();
        drop(env);

        for (database, key, value) in [
            (b"kv".as_slice(), b"alice".as_slice(), None),
            (b"dup", b"k", Some(b"b".as_slice())),
            (b"v4", b"192.0.2.0/24", None),
            (b"v6", b"2001:db8:1::/48", None),
        ] {
            let before = Reader::open(&directory.0).unwrap();
            let target = selected(&before, database, key, value);
            let sequence =
                apply(&directory.0, &target, Action::Replace(b"edited".to_vec())).unwrap();
            let after = Reader::open(&directory.0).unwrap();
            assert_eq!(sequence, after.sequence());
            let target = selected(&after, database, key, Some(b"edited"));
            apply(&directory.0, &target, Action::Delete).unwrap();
        }
        assert_eq!(
            contents(&directory.0, b"kv"),
            [(b"bob".to_vec(), b"Bob".to_vec())]
        );
        assert_eq!(
            contents(&directory.0, b"dup"),
            [
                (b"k".to_vec(), b"a".to_vec()),
                (b"k".to_vec(), b"c".to_vec())
            ]
        );
        for database in [b"v4", b"v6"] {
            let values: Vec<_> = contents(&directory.0, database)
                .into_iter()
                .map(|(_, value)| value)
                .collect();
            assert_eq!(values, [b"cover".to_vec(), b"child".to_vec()]);
        }
    }

    #[test]
    fn large_values_use_full_bytes_and_large_duplicate_identity_is_unambiguous() {
        let directory = Directory::new();
        let env = Environment::create(&directory.0).unwrap();
        let mut write = env.begin_write().unwrap();
        write.create_database(b"kv", DatabaseKind::Kv).unwrap();
        write
            .create_database(b"dup", DatabaseKind::Duplicates { width: None })
            .unwrap();
        let mut value = vec![b'a'; 6000];
        value[5000] = 255;
        write.put(b"kv", b"large", &value).unwrap();
        let mut dup1 = vec![b'x'; 6000];
        dup1[5999] = b'a';
        let mut dup2 = dup1.clone();
        dup2[5999] = b'b';
        write
            .insert_duplicate(b"dup", b"same-prefix", &dup1)
            .unwrap();
        write
            .insert_duplicate(b"dup", b"same-prefix", &dup2)
            .unwrap();
        write.commit().unwrap();
        drop(env);
        let reader = Reader::open(&directory.0).unwrap();
        let row = &rows(&reader, b"kv")[0];
        assert_eq!(row.value.len(), VALUE_PREVIEW_LIMIT);
        assert!(row.duplicate_value.is_none());
        let target = reader.target(b"kv", row).unwrap();
        assert_eq!(target.old_value, value);
        let mut replacement = target.old_value.clone();
        replacement[0] = b'z';
        apply(&directory.0, &target, Action::Replace(replacement.clone())).unwrap();
        assert_eq!(
            ReadTxn::open_snapshot(&directory.0)
                .unwrap()
                .get(b"kv", b"large")
                .unwrap(),
            Some(replacement.as_slice())
        );

        let reader = Reader::open(&directory.0).unwrap();
        let rows = rows(&reader, b"dup");
        assert_eq!(rows[0].value, rows[1].value);
        let target = reader.target(b"dup", &rows[1]).unwrap();
        assert_eq!(target.old_value, dup2);
        let mut dup3 = dup2.clone();
        dup3[5999] = b'c';
        apply(&directory.0, &target, Action::Replace(dup3.clone())).unwrap();
        let read = ReadTxn::open_snapshot(&directory.0).unwrap();
        assert_eq!(read.duplicate_count(b"dup", b"same-prefix").unwrap(), 2);
        assert_eq!(
            read.duplicate_seek_pair(b"dup", b"same-prefix", &dup1)
                .unwrap(),
            Some(dup1.as_slice())
        );
        assert_eq!(
            read.duplicate_seek_pair(b"dup", b"same-prefix", &dup3)
                .unwrap(),
            Some(dup3.as_slice())
        );
    }

    #[test]
    fn stale_targets_and_duplicate_collisions_do_not_commit_changes() {
        let directory = Directory::new();
        let env = Environment::create(&directory.0).unwrap();
        let mut write = env.begin_write().unwrap();
        write.create_database(b"kv", DatabaseKind::Kv).unwrap();
        write
            .create_database(b"fixed", DatabaseKind::Duplicates { width: Some(3) })
            .unwrap();
        write.put(b"kv", b"k", b"old").unwrap();
        write.insert_duplicate(b"fixed", b"k", b"one").unwrap();
        write.insert_duplicate(b"fixed", b"k", b"two").unwrap();
        write.commit().unwrap();
        drop(env);
        let reader = Reader::open(&directory.0).unwrap();
        let target = selected(&reader, b"fixed", b"k", Some(b"one"));
        let before = std::fs::read(directory.0.join("CURRENT")).unwrap();
        let mut foreign = target.clone();
        foreign.uuid[0] ^= 1;
        assert!(
            apply(&directory.0, &foreign, Action::Delete)
                .unwrap_err()
                .contains("changed since")
        );
        assert!(
            apply(&directory.0, &target, Action::Replace(b"two".to_vec()))
                .unwrap_err()
                .contains("already exists")
        );
        assert!(
            apply(
                &directory.0,
                &target,
                Action::Replace(b"bad width".to_vec())
            )
            .unwrap_err()
            .contains("exactly 3")
        );
        assert_eq!(std::fs::read(directory.0.join("CURRENT")).unwrap(), before);
        assert_eq!(
            contents(&directory.0, b"fixed"),
            [
                (b"k".to_vec(), b"one".to_vec()),
                (b"k".to_vec(), b"two".to_vec())
            ]
        );
        assert_eq!(
            apply(&directory.0, &target, Action::Replace(b"one".to_vec())).unwrap(),
            reader.sequence()
        );
        assert_eq!(std::fs::read(directory.0.join("CURRENT")).unwrap(), before);

        let stale = selected(&reader, b"kv", b"k", None);
        let env = Environment::open(&directory.0).unwrap();
        let mut write = env.begin_write().unwrap();
        write.put(b"kv", b"other", b"new commit").unwrap();
        write.commit().unwrap();
        drop(env);
        assert!(
            apply(&directory.0, &stale, Action::Replace(b"overwrite".to_vec()))
                .unwrap_err()
                .contains("changed since")
        );
        assert!(
            apply(&directory.0, &stale, Action::Delete)
                .unwrap_err()
                .contains("changed since")
        );
        assert_eq!(
            ReadTxn::open_snapshot(&directory.0)
                .unwrap()
                .get(b"kv", b"k")
                .unwrap(),
            Some(b"old".as_slice())
        );
    }

    #[test]
    fn over_limit_records_are_never_truncated_into_mutation_targets() {
        let directory = Directory::new();
        let env = Environment::create(&directory.0).unwrap();
        let mut write = env.begin_write().unwrap();
        write.create_database(b"kv", DatabaseKind::Kv).unwrap();
        write
            .put(b"kv", b"huge", &vec![b'a'; MAX_EDIT_BYTES + 1])
            .unwrap();
        write.commit().unwrap();
        drop(env);
        let reader = Reader::open(&directory.0).unwrap();
        let before = std::fs::read(directory.0.join("CURRENT")).unwrap();
        let row = &rows(&reader, b"kv")[0];
        assert!(reader.target(b"kv", row).unwrap_err().contains("64 KiB"));
        assert_eq!(std::fs::read(directory.0.join("CURRENT")).unwrap(), before);
    }

    #[test]
    fn active_writer_is_reported_as_busy_without_changing_record() {
        let directory = Directory::new();
        let env = Environment::create(&directory.0).unwrap();
        let mut write = env.begin_write().unwrap();
        write.create_database(b"kv", DatabaseKind::Kv).unwrap();
        write.put(b"kv", b"k", b"old").unwrap();
        write.commit().unwrap();
        let reader = Reader::open(&directory.0).unwrap();
        let target = selected(&reader, b"kv", b"k", None);
        assert!(
            apply(&directory.0, &target, Action::Replace(b"new".to_vec()))
                .unwrap_err()
                .contains("writer")
        );
        assert_eq!(
            env.begin_read().get(b"kv", b"k").unwrap(),
            Some(b"old".as_slice())
        );
    }

    #[test]
    fn cohort_and_indexed_edits_and_deletes_use_their_native_publication_paths() {
        for mode in [
            ulmdb::PublicationMode::Cohort,
            ulmdb::PublicationMode::Indexed,
        ] {
            let directory = Directory::new();
            let env = Environment::create_with_publication(&directory.0, mode).unwrap();
            let mut write = env.begin_write().unwrap();
            write.create_database(b"kv", DatabaseKind::Kv).unwrap();
            write.put(b"kv", b"key", b"old").unwrap();
            write.commit().unwrap();
            drop(env);
            let reader = Reader::open(&directory.0).unwrap();
            let target = selected(&reader, b"kv", b"key", None);
            let sequence = apply(&directory.0, &target, Action::Replace(b"new".to_vec())).unwrap();
            let reader = Reader::open(&directory.0).unwrap();
            assert_eq!(reader.sequence(), sequence);
            let target = selected(&reader, b"kv", b"key", Some(b"new"));
            apply(&directory.0, &target, Action::Delete).unwrap();
            assert!(contents(&directory.0, b"kv").is_empty());
        }
    }
}
