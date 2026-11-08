//! Real sealed Native D/P fixtures. No selector rewrite is used to inspect P.
use super::*;
use ulmdb::{DatabaseKind, PublicationMode};

const MODES: [PublicationMode; 3] = [
    PublicationMode::Legacy,
    PublicationMode::Cohort,
    PublicationMode::Indexed,
];

struct Fixture(PathBuf);
impl Fixture {
    fn new() -> Self {
        let parent = std::env::var_os("ULMDB_ARCHIVE_V2_EVIDENCE")
            .map(PathBuf::from)
            .unwrap_or_else(std::env::temp_dir);
        fs::create_dir_all(&parent).unwrap();
        let path = parent.join(format!(
            "ulmdb-archive-pair-{}-{}",
            std::process::id(),
            ulmdb_storage::new_identity().unwrap()
        ));
        fs::create_dir(&path).unwrap();
        Self(path)
    }
}
impl Drop for Fixture {
    fn drop(&mut self) {
        if std::env::var_os("ULMDB_ARCHIVE_V2_EVIDENCE").is_none() {
            let _ = fs::remove_dir_all(&self.0);
        }
    }
}
fn source(fixture: &Fixture, name: &str, mode: PublicationMode) -> PathBuf {
    let path = fixture.0.join(name);
    let env = Environment::create_with_publication(&path, mode).unwrap();
    let mut writer = env.begin_write().unwrap();
    writer.create_database(b"data", DatabaseKind::Kv).unwrap();
    writer
        .create_database(b"duplicates", DatabaseKind::Duplicates { width: Some(4) })
        .unwrap();
    writer.put(b"data", b"key", b"A").unwrap();
    writer
        .insert_duplicate(b"duplicates", b"key", b"old1")
        .unwrap();
    writer.commit().unwrap();
    let mut writer = env.begin_write().unwrap();
    writer.put(b"data", b"key", b"B").unwrap();
    writer
        .insert_duplicate(b"duplicates", b"key", b"new2")
        .unwrap();
    writer.commit().unwrap();
    drop(env);
    fs::write(path.join("LMDB-ABI"), b"1.0\n").unwrap();
    fs::write(path.join("LMDB-MAPSIZE"), (16u64 << 20).to_le_bytes()).unwrap();
    path
}
fn archive(path: &Path) -> Vec<u8> {
    let mut bytes = Vec::new();
    write_archive_v2(path, &mut bytes).unwrap();
    assert_eq!(&bytes[8..12], &2u32.to_le_bytes());
    bytes
}
fn records(bytes: &[u8]) -> Vec<(String, Vec<u8>)> {
    let count = u64::from_le_bytes(bytes[16..24].try_into().unwrap());
    let mut offset = 32;
    (0..count)
        .map(|_| {
            let length = u32::from_le_bytes(bytes[offset..offset + 4].try_into().unwrap()) as usize;
            let size =
                u64::from_le_bytes(bytes[offset + 4..offset + 12].try_into().unwrap()) as usize;
            offset += 12;
            let name = String::from_utf8(bytes[offset..offset + length].to_vec()).unwrap();
            offset += length;
            let data = bytes[offset..offset + size].to_vec();
            offset += size;
            (name, data)
        })
        .collect()
}
fn frame(version: u32, records: &[(String, Vec<u8>)]) -> Vec<u8> {
    let mut bytes = Vec::new();
    bytes.extend_from_slice(ARCHIVE_MAGIC);
    bytes.extend_from_slice(&version.to_le_bytes());
    bytes.extend_from_slice(&0u32.to_le_bytes());
    bytes.extend_from_slice(&(records.len() as u64).to_le_bytes());
    bytes.extend_from_slice(
        &records
            .iter()
            .map(|(_, v)| v.len() as u64)
            .sum::<u64>()
            .to_le_bytes(),
    );
    for (name, data) in records {
        bytes.extend_from_slice(&(name.len() as u32).to_le_bytes());
        bytes.extend_from_slice(&(data.len() as u64).to_le_bytes());
        bytes.extend_from_slice(name.as_bytes());
        bytes.extend_from_slice(data);
    }
    bytes
}
fn reject(fixture: &Fixture, name: &str, bytes: &[u8]) {
    let input = fixture.0.join(format!("{name}.archive"));
    let target = fixture.0.join(format!("{name}-restored"));
    fs::write(&input, bytes).unwrap();
    assert!(restore_archive(&input, &target).is_err(), "{name}");
    assert!(!target.exists(), "{name} published a target");
    assert!(!fixture.0.join("escape").exists());
    assert!(
        !fs::read_dir(&fixture.0).unwrap().any(|e| {
            e.unwrap()
                .file_name()
                .to_string_lossy()
                .starts_with(".ulmdb-copy-")
        }),
        "{name} leaked its owned staging directory"
    );
}

#[test]
fn strong_v2_pair_round_trip_preserves_both_roots_and_independent_writer() {
    for mode in MODES {
        let fixture = Fixture::new();
        let path = source(&fixture, "source", mode);
        let original = ulmdb_storage::read_roots(&path).unwrap();
        let bytes = archive(&path);
        let input = fixture.0.join("pair.archive");
        let target = fixture.0.join("restored");
        fs::write(&input, &bytes).unwrap();
        restore_archive(&input, &target).unwrap();
        fs::write(
            fixture.0.join("successful-pair-mode"),
            format!("{mode:?}\n"),
        )
        .unwrap();
        assert_eq!(
            fs::read(path.join("CURRENT")).unwrap(),
            fs::read(target.join("CURRENT")).unwrap()
        );
        assert_eq!(fs::read(target.join("LMDB-ABI")).unwrap(), b"1.0\n");
        let env = Environment::open(&target).unwrap();
        ulmdb::backup::verify(&env).unwrap();
        let current = env.begin_read();
        assert_eq!(current.sequence(), 2);
        assert_eq!(current.get(b"data", b"key").unwrap(), Some(&b"B"[..]));
        assert_eq!(current.duplicate_count(b"duplicates", b"key").unwrap(), 2);
        let previous =
            ReadTxn::open_previous_coordinated_snapshot(&target, ulmdb::ResidencyPolicy::Demand)
                .unwrap();
        assert_eq!(previous.sequence(), 1);
        assert_eq!(previous.get(b"data", b"key").unwrap(), Some(&b"A"[..]));
        assert_eq!(previous.duplicate_count(b"duplicates", b"key").unwrap(), 1);
        assert_eq!(
            previous.file_checkpoint().unwrap(),
            ulmdb_storage::checkpoint_bytes(original.1.as_ref().unwrap()).unwrap()
        );
        let mut writer = env.begin_write().unwrap();
        writer.put(b"data", b"key", b"C").unwrap();
        writer.commit().unwrap();
        assert_eq!(current.get(b"data", b"key").unwrap(), Some(&b"B"[..]));
        assert_eq!(previous.get(b"data", b"key").unwrap(), Some(&b"A"[..]));
        drop(previous);
        drop(current);
        drop(env);
        let original = Environment::open(&path).unwrap();
        assert_eq!(original.begin_read().sequence(), 2);
        assert_eq!(
            original.begin_read().get(b"data", b"key").unwrap(),
            Some(&b"B"[..])
        );
    }
}

#[test]
fn v2_rejects_bad_frames_missing_and_foreign_predecessors_before_publish() {
    for mode in MODES {
        let fixture = Fixture::new();
        let path = source(&fixture, "source", mode);
        let valid = archive(&path);
        let original = records(&valid);
        let current = original
            .iter()
            .find(|(n, _)| n == "CURRENT")
            .unwrap()
            .1
            .clone();
        for (name, replacement) in [
            ("traversal", "../escape"),
            ("absolute", "/escape"),
            ("backslash", "segments\\escape"),
            ("subdirectory", "commits/../escape"),
            ("unknown", "LOCK"),
            ("empty-name", ""),
        ] {
            let mut changed = original.clone();
            changed[0].0 = replacement.to_string();
            reject(&fixture, name, &frame(2, &changed));
        }
        for name in [
            "CURRENT",
            original
                .iter()
                .find(|(n, _)| n.contains('/'))
                .unwrap()
                .0
                .as_str(),
        ] {
            let mut changed = original.clone();
            changed.push(changed.iter().find(|(n, _)| n == name).unwrap().clone());
            reject(
                &fixture,
                &format!(
                    "duplicate-{}",
                    if name == "CURRENT" {
                        "current"
                    } else {
                        "asset"
                    }
                ),
                &frame(2, &changed),
            );
        }
        let no_current: Vec<_> = original
            .iter()
            .filter(|(n, _)| n != "CURRENT")
            .cloned()
            .collect();
        reject(&fixture, "no-current", &frame(2, &no_current));
        let mut no_previous = original.clone();
        if current.len() == 64 {
            no_previous.retain(|(n, _)| n != "PREVIOUS");
            let mut duplicate = original.clone();
            duplicate.push(
                duplicate
                    .iter()
                    .find(|(n, _)| n == "PREVIOUS")
                    .unwrap()
                    .clone(),
            );
            reject(&fixture, "duplicate-previous", &frame(2, &duplicate));
            // The original v1 grammar remains strict, including when its file
            // bytes would otherwise describe a valid pair.
            reject(&fixture, "v1-forbids-previous", &frame(1, &original));
        } else {
            let pointer = no_previous
                .iter_mut()
                .find(|(n, _)| n == "CURRENT")
                .unwrap();
            pointer.1[12..16].copy_from_slice(&0u32.to_le_bytes());
            pointer.1[80..144].fill(0);
            let mut ancillary = original.clone();
            ancillary.push(("PREVIOUS".into(), current[80..144].to_vec()));
            reject(&fixture, "bundled-forbids-ancillary", &frame(2, &ancillary));
        }
        reject(&fixture, "no-previous", &frame(2, &no_previous));
        let other = source(&fixture, "foreign", mode);
        let other_records = records(&archive(&other));
        let foreign =
            ulmdb_storage::checkpoint_bytes(&ulmdb_storage::read_current(&other).unwrap()).unwrap();
        let mut mixed = original.clone();
        if current.len() == 64 {
            mixed.iter_mut().find(|(n, _)| n == "PREVIOUS").unwrap().1 = foreign.to_vec();
        } else {
            mixed.iter_mut().find(|(n, _)| n == "CURRENT").unwrap().1[80..144]
                .copy_from_slice(&foreign);
        }
        mixed.extend(other_records.into_iter().filter(|(n, _)| n.contains('/')));
        reject(&fixture, "foreign-valid-assets", &frame(2, &mixed));
        for version in [1, 2] {
            let mut changed = original.clone();
            changed
                .iter_mut()
                .find(|(n, _)| n == "CURRENT")
                .unwrap()
                .1
                .resize(176, 0);
            reject(
                &fixture,
                &format!("weak176-v{version}"),
                &frame(version, &changed),
            );
        }
        let mut bad_crc = original.clone();
        let bytes = &mut bad_crc.iter_mut().find(|(n, _)| n == "CURRENT").unwrap().1;
        let offset = if bytes.len() == 64 { 63 } else { 79 };
        bytes[offset] ^= 1;
        reject(&fixture, "bad-selector-crc", &frame(2, &bad_crc));
        let mut truncated = valid.clone();
        truncated.pop();
        reject(&fixture, "truncated", &truncated);
        let mut trailing = valid.clone();
        trailing.push(0);
        reject(&fixture, "trailing", &trailing);
        let mut wrong_total = valid.clone();
        wrong_total[24..32].copy_from_slice(&0u64.to_le_bytes());
        reject(&fixture, "wrong-total", &wrong_total);
        let mut unsupported = valid.clone();
        unsupported[8..12].copy_from_slice(&3u32.to_le_bytes());
        reject(&fixture, "future-version", &unsupported);
        for (name, offset, replacement) in [
            ("no-files", 16, 0u64.to_le_bytes().to_vec()),
            ("too-many-files", 16, (MAX_FILES + 1).to_le_bytes().to_vec()),
            ("too-many-bytes", 24, (MAX_TOTAL + 1).to_le_bytes().to_vec()),
            ("oversized-name", 32, 129u32.to_le_bytes().to_vec()),
            ("oversized-file", 36, (MAX_FILE + 1).to_le_bytes().to_vec()),
            ("reserved-header", 12, 1u32.to_le_bytes().to_vec()),
        ] {
            let mut changed = valid.clone();
            changed[offset..offset + replacement.len()].copy_from_slice(&replacement);
            reject(&fixture, name, &changed);
        }
        let mut bad_utf8 = valid.clone();
        bad_utf8[44] = 0xff;
        reject(&fixture, "non-utf8-name", &bad_utf8);
        let destination = fixture.0.join("existing");
        fs::create_dir(&destination).unwrap();
        fs::write(destination.join("keep"), b"untouched").unwrap();
        let input = fixture.0.join("valid.archive");
        fs::write(&input, valid).unwrap();
        assert!(restore_archive(&input, &destination).is_err());
        assert_eq!(fs::read(destination.join("keep")).unwrap(), b"untouched");
    }
}

// Create A and B from separate sealed inventories. A-only malformed user data
// must be rejected by full predecessor admission, not by the current inventory
// hashes/catalog decoder. This is a corruption fixture, never a native API use
// presented as a valid business publication.
fn sealed_root(
    path: &Path,
    base: &ulmdb_storage::Manifest,
    bad: bool,
    epoch: u64,
    wrong_uuid: bool,
) -> ulmdb_storage::Manifest {
    use ulmdb_storage::{Arena, Reference, SegmentTable, WriteStore};
    let mut arena = Arena::new(SegmentTable::default(), base.next_segment).unwrap();
    let root = if bad {
        arena.allocate(&[0; 4096], 4096).unwrap()
    } else {
        ulmdb_kv::put(&mut arena, Reference::NULL, b"key", b"valid").unwrap()
    };
    let mut db = [0; 80];
    db[..8].copy_from_slice(&1u64.to_le_bytes());
    db[8..10].copy_from_slice(&1u16.to_le_bytes());
    db[10..12].copy_from_slice(&1u16.to_le_bytes());
    db[16..32].copy_from_slice(&root.encode());
    db[32..40].copy_from_slice(&4096u64.to_le_bytes());
    db[40..48].copy_from_slice(&67_108_864u64.to_le_bytes());
    db[48..56].copy_from_slice(&65_536u64.to_le_bytes());
    db[56..64].copy_from_slice(&u64::MAX.to_le_bytes());
    let catalog = ulmdb_kv::put(&mut arena, Reference::NULL, b"data", &db).unwrap();
    let (table, _) = arena.seal(path).unwrap();
    let entries = table.entries();
    let mut manifest = base.clone();
    manifest.generation = ulmdb_storage::new_identity().unwrap();
    manifest.epoch = epoch;
    if wrong_uuid {
        manifest.uuid[0] ^= 1;
    }
    manifest.sequence += 1;
    manifest.commit = manifest.generation;
    manifest.parent = base.commit;
    manifest.previous = base.generation;
    manifest.catalog = catalog;
    manifest.next_database = 2;
    manifest.next_segment = entries.last().unwrap().id + 1;
    manifest.segments = entries.into();
    if manifest.format_minor == 4 {
        manifest.prepare_indexed(None).unwrap();
    }
    ulmdb_storage::publish(path, &manifest).unwrap();
    manifest
}

#[test]
fn v2_authenticates_predecessor_user_graph_and_namespace_before_publish() {
    for mode in MODES {
        for fault in 0..3 {
            let bad_tree = fault == 0;
            let fixture = Fixture::new();
            let path = fixture.0.join("source");
            drop(Environment::create_with_publication(&path, mode).unwrap());
            let genesis = ulmdb_storage::read_current(&path).unwrap();
            let a = sealed_root(&path, &genesis, bad_tree, 1, false);
            // Valid B has no A inventory, so its ordinary complete audit passes.
            let b = sealed_root(&path, &a, false, if fault == 1 { 2 } else { 1 }, fault == 2);
            let env = Environment::open(&path).unwrap();
            ulmdb::backup::verify(&env).unwrap();
            assert_eq!(env.begin_read().sequence(), b.sequence);
            assert_eq!(
                env.begin_read().get(b"data", b"key").unwrap(),
                Some(&b"valid"[..])
            );
            drop(env);
            let mut refused = Vec::new();
            assert!(write_archive_v2(&path, &mut refused).is_err());
            assert!(refused.is_empty(), "invalid pair reached output stream");
            let mut forged = Vec::new();
            write_version(Version::Pair, &path, &mut forged).unwrap();
            reject(
                &fixture,
                match fault {
                    0 => "sealed-bad-old-user-tree",
                    1 => "foreign-old-epoch",
                    _ => "foreign-old-uuid",
                },
                &forged,
            );
        }
    }
}

#[test]
fn v2_write_errors_are_reported_and_selector_symlinks_are_rejected() {
    struct Failing;
    impl Write for Failing {
        fn write(&mut self, _: &[u8]) -> std::io::Result<usize> {
            Err(std::io::Error::from_raw_os_error(libc::ENOSPC))
        }
        fn flush(&mut self) -> std::io::Result<()> {
            Ok(())
        }
    }
    let fixture = Fixture::new();
    let path = source(&fixture, "source", PublicationMode::Legacy);
    assert_eq!(write_archive_v2(&path, &mut Failing), Err(libc::ENOSPC));
    #[cfg(unix)]
    {
        let bytes = fs::read(path.join("PREVIOUS")).unwrap();
        fs::rename(path.join("PREVIOUS"), path.join("previous-original")).unwrap();
        std::os::unix::fs::symlink("previous-original", path.join("PREVIOUS")).unwrap();
        assert_eq!(fs::read(path.join("PREVIOUS")).unwrap(), bytes);
        assert!(write_archive_v2(&path, &mut Vec::new()).is_err());
    }
}
