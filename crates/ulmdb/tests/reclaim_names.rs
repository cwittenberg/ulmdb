//! Observable filename policy and typed mark deduplication, across all modes.
use std::{
    collections::BTreeSet,
    fs,
    path::{Path, PathBuf},
};
use ulmdb::{DatabaseKind, Environment, Error, PublicationMode};
use ulmdb_storage::Manifest;
struct Directory(PathBuf);
impl Directory {
    fn new() -> Self {
        Self(std::env::temp_dir().join(format!(
            "ulmdb-reclaim-names-{}",
            ulmdb_storage::new_identity().unwrap()
        )))
    }
    fn child(&self, name: &str) -> PathBuf {
        self.0.join(name)
    }
}
impl Drop for Directory {
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.0);
    }
}
fn initialize(d: &Directory, mode: PublicationMode) -> Environment {
    let env = Environment::create_with_publication(&d.0, mode).unwrap();
    let mut w = env.begin_write().unwrap();
    w.create_database(b"kv", DatabaseKind::Kv).unwrap();
    w.put(b"kv", b"key", b"first").unwrap();
    w.commit().unwrap();
    env
}
fn write(path: PathBuf, bytes: &[u8], list: &mut Vec<PathBuf>) {
    fs::write(&path, bytes).unwrap();
    list.push(path);
}
#[test]
fn only_exact_lowercase_fixed_width_names_are_candidates() {
    let d = Directory::new();
    let env = initialize(&d, PublicationMode::Legacy);
    let mut remove = Vec::new();
    let mut keep = Vec::new();
    for (folder, width, suffix) in [
        ("segments", 64, "seg"),
        ("commits", 32, "root"),
        ("cohorts", 32, "cohort"),
    ] {
        // Zero and nonzero IDs, with every required leading zero, are distinct
        // canonical names. Width/suffix variants never alias these identities.
        for value in ['0', 'a'] {
            write(
                d.child(folder)
                    .join(format!("{}{value}.{suffix}", "0".repeat(width - 1))),
                b"orphan",
                &mut remove,
            );
        }
        for name in [
            format!("a.{suffix}"),
            format!("{}a.{suffix}", "0".repeat(width)),
            format!("{}.{}", "a".repeat(width), suffix.to_uppercase()),
            format!("{}.{}", "A".repeat(width), suffix),
            format!("{}.{}.bak", "a".repeat(width), suffix),
            format!("é{}.{}", "0".repeat(width - 2), suffix),
        ] {
            write(d.child(folder).join(name), b"unknown spelling", &mut keep);
        }
        write(
            d.child(folder).join(".prepare-.tmp"),
            b"private",
            &mut remove,
        );
        write(
            d.child(folder).join(".prepare-x.tmp.bak"),
            b"unknown temporary spelling",
            &mut keep,
        );
    }
    // An uppercase hard-link alias is unknown and must survive removing the
    // canonical orphan, even though both directory entries share an inode.
    let canonical = &remove[0];
    let alias = d.child("segments").join(format!("{}.seg", "F".repeat(64)));
    fs::hard_link(canonical, &alias).unwrap();
    keep.push(alias);
    write(
        d.child("transactions/operator-note.txt"),
        b"private namespace",
        &mut remove,
    );
    write(d.child(".prepare-root.tmp"), b"private root", &mut remove);
    fs::create_dir(d.child("retained")).unwrap();
    write(
        d.child("retained/.prepare-pin.tmp"),
        b"private pin",
        &mut remove,
    );
    let expected_bytes = remove
        .iter()
        .map(|p| fs::metadata(p).unwrap().len())
        .sum::<u64>();
    let report = env.reclaim().unwrap();
    assert_eq!(
        (
            report.removed_segments,
            report.removed_descriptors,
            report.removed_cohorts,
            report.removed_temporary_files
        ),
        (2, 2, 2, 6)
    );
    assert_eq!(report.removed_bytes, expected_bytes);
    for path in remove {
        assert!(
            !path.exists(),
            "canonical/private file was retained: {}",
            path.display()
        );
    }
    for path in keep {
        assert!(
            path.exists(),
            "unknown alias was removed: {}",
            path.display()
        );
    }
    assert_eq!(
        env.begin_read().get(b"kv", b"key").unwrap(),
        Some(b"first".as_slice())
    );
}
#[cfg(unix)]
#[test]
fn nonregular_and_nonutf8_candidates_fail_before_any_unlink() {
    use std::{
        ffi::OsString,
        os::unix::{ffi::OsStringExt, fs::symlink},
    };
    let d = Directory::new();
    let env = initialize(&d, PublicationMode::Legacy);
    let orphan = d.child("segments").join(format!("{}.seg", "b".repeat(64)));
    fs::write(&orphan, b"eligible orphan").unwrap();
    let bad = d.child("cohorts").join(format!("{:032x}.cohort", 0xabc));
    symlink(&orphan, &bad).unwrap();
    assert!(matches!(env.reclaim(), Err(Error::Corrupt(_))));
    assert!(orphan.exists());
    fs::remove_file(&bad).unwrap();
    fs::create_dir(&bad).unwrap();
    assert!(matches!(env.reclaim(), Err(Error::Corrupt(_))));
    assert!(orphan.exists());
    fs::remove_dir(&bad).unwrap();
    let nonutf8 = d.child("segments").join(OsString::from_vec(vec![0xff]));
    fs::write(&nonutf8, b"unknown bytes").unwrap();
    assert!(matches!(env.reclaim(), Err(Error::Corrupt(_))));
    assert!(orphan.exists());
    fs::remove_file(nonutf8).unwrap();
    let unknown_link = d.child("cohorts/ABC.cohort");
    symlink(&orphan, &unknown_link).unwrap();
    env.reclaim().unwrap();
    assert!(!orphan.exists());
    assert!(
        fs::symlink_metadata(unknown_link)
            .unwrap()
            .file_type()
            .is_symlink()
    );
}
fn physical(directory: &Path, manifest: &Manifest) -> BTreeSet<PathBuf> {
    let mut files = manifest
        .segments
        .iter()
        .map(|e| ulmdb_storage::segment_path(directory, e))
        .collect::<BTreeSet<_>>();
    files.insert(ulmdb_storage::descriptor_path(directory, manifest));
    files.extend(
        manifest
            .metadata_origins()
            .into_iter()
            .map(|id| directory.join("cohorts").join(format!("{id:032x}.cohort"))),
    );
    files
}
#[test]
fn duplicate_pins_and_readers_count_each_physical_identity_once() {
    for mode in [
        PublicationMode::Legacy,
        PublicationMode::Cohort,
        PublicationMode::Indexed,
    ] {
        let d = Directory::new();
        let env = initialize(&d, mode);
        let old = env.begin_read();
        let first = ulmdb_storage::read_current(&d.0).unwrap();
        env.retain_snapshot("alpha", &old).unwrap();
        env.retain_snapshot("beta", &old).unwrap();
        for value in [b"second".as_slice(), b"third", b"fourth"] {
            let mut w = env.begin_write().unwrap();
            w.put(b"kv", b"key", value).unwrap();
            w.commit().unwrap();
        }
        let (current, previous) = ulmdb_storage::read_roots(&d.0).unwrap();
        let previous = previous.unwrap();
        let mut expected = physical(&d.0, &current);
        expected.extend(physical(&d.0, &previous));
        expected.extend(physical(&d.0, &first));
        let report = env.reclaim().unwrap();
        assert_eq!(report.retained_roots, 3);
        assert_eq!(
            report.retained_segment_files,
            expected
                .iter()
                .filter(|p| p.parent() == Some(d.child("segments").as_path()))
                .count()
        );
        assert_eq!(
            report.retained_cohort_files,
            expected
                .iter()
                .filter(|p| p.parent() == Some(d.child("cohorts").as_path()))
                .count()
        );
        env.release_snapshot("alpha").unwrap();
        let without_duplicate = env.reclaim().unwrap();
        assert_eq!(
            (
                without_duplicate.retained_roots,
                without_duplicate.retained_segment_files,
                without_duplicate.retained_cohort_files
            ),
            (
                report.retained_roots,
                report.retained_segment_files,
                report.retained_cohort_files
            )
        );
        env.release_snapshot("beta").unwrap();
        let live_only = env.reclaim().unwrap();
        assert_eq!(live_only.retained_roots, 3);
        assert_eq!(old.get(b"kv", b"key").unwrap(), Some(b"first".as_slice()));
        drop(old);
        assert_eq!(env.reclaim().unwrap().retained_roots, 2);
    }
}

#[test]
fn leading_zero_generation_is_kept_without_retaining_other_namespaces() {
    for minor in [2, 3, 4] {
        let d = Directory::new();
        ulmdb_storage::create_layout(&d.0).unwrap();
        let mut manifest = Manifest::empty().unwrap();
        manifest.format_minor = minor;
        manifest.generation = 0x0a;
        if minor == 4 {
            manifest.prepare_indexed(None).unwrap();
        }
        ulmdb_storage::publish(&d.0, &manifest).unwrap();
        let env = Environment::open(&d.0).unwrap();
        let selected = ulmdb_storage::descriptor_path(&d.0, &manifest);
        let (own_directory, own_suffix, other_directory, other_suffix) = if minor == 2 {
            ("commits", "root", "cohorts", "cohort")
        } else {
            ("cohorts", "cohort", "commits", "root")
        };
        let other = d
            .child(other_directory)
            .join(format!("{:032x}.{other_suffix}", 0x0a));
        let swapped = d
            .child(own_directory)
            .join(format!("{:032x}.{own_suffix}", 0x0a00));
        let hash = d
            .child("segments")
            .join(format!("{}0a.seg", "0".repeat(62)));
        for orphan in [&other, &swapped, &hash] {
            fs::write(orphan, b"unreferenced identity").unwrap();
        }
        let report = env.reclaim().unwrap();
        assert_eq!(
            (
                report.removed_segments,
                report.removed_descriptors,
                report.removed_cohorts
            ),
            (1, 1, 1)
        );
        assert_eq!(report.retained_roots, 1);
        assert!(selected.exists());
        for orphan in [&other, &swapped, &hash] {
            assert!(!orphan.exists());
        }
        assert_eq!(ulmdb_storage::read_current(&d.0).unwrap().generation, 0x0a);
    }
}

#[test]
fn head_drift_poisoning_precedes_corrupt_predecessor_validation() {
    for mode in [
        PublicationMode::Legacy,
        PublicationMode::Cohort,
        PublicationMode::Indexed,
    ] {
        let d = Directory::new();
        let env = initialize(&d, mode);
        let held = env.begin_read();
        let before = ulmdb_storage::read_current(&d.0).unwrap();
        let mut drift = before.clone();
        drift.generation = ulmdb_storage::new_identity().unwrap();
        drift.previous = before.generation;
        if mode == PublicationMode::Indexed {
            // The selected root must decode independently of the corrupt
            // predecessor, otherwise a selected-root decode error comes first.
            drift.prepare_indexed(None).unwrap();
        }
        ulmdb_storage::publish(&d.0, &drift).unwrap();
        let prior_path = ulmdb_storage::descriptor_path(&d.0, &before);
        // Moving the original inode preserves existing mapped reader bytes.
        // Only the now-stale canonical predecessor path is replaced.
        fs::rename(&prior_path, d.child("preserved-prior-root")).unwrap();
        fs::write(&prior_path, b"corrupt predecessor descriptor").unwrap();
        let orphan = d.child("segments").join(format!("{}.seg", "b".repeat(64)));
        fs::write(&orphan, b"eligible but must not unlink").unwrap();
        let outcome = env.reclaim();
        assert!(
            matches!(outcome, Err(Error::OutcomeUnknown)),
            "{mode:?}: {outcome:?}"
        );
        assert!(orphan.exists());
        assert!(matches!(env.begin_write(), Err(Error::OutcomeUnknown)));
        assert_eq!(held.get(b"kv", b"key").unwrap(), Some(b"first".as_slice()));
    }
}
