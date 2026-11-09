use super::*;
use crate::{MaintenancePolicy, PublicationMode};

struct Directory(PathBuf);
impl Directory {
    fn new() -> Self {
        Self(std::env::temp_dir().join(format!(
            "ulmdb-backup-archive-{}",
            ulmdb_storage::new_identity().unwrap()
        )))
    }
}
impl Drop for Directory {
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.0);
    }
}

fn archived_origin() -> (Directory, Environment, PathBuf, PathBuf, Vec<u8>) {
    let dir = Directory::new();
    let env = Environment::create_with_publication(&dir.0, PublicationMode::Indexed).unwrap();
    env.set_maintenance_policy(MaintenancePolicy::Disabled)
        .unwrap();
    let mut w = env.begin_write().unwrap();
    w.create_database(b"kv", DatabaseKind::Kv).unwrap();
    w.commit().unwrap();
    for n in 0..6 {
        let mut w = env.begin_write().unwrap();
        w.put(b"kv", b"obsolete", &[n; 12_000]).unwrap();
        w.commit().unwrap();
    }
    for n in 0..18u64 {
        let mut w = env.begin_write().unwrap();
        w.put(b"kv", &n.to_be_bytes(), &[n as u8; 12_000]).unwrap();
        w.commit().unwrap();
    }
    assert!(env.prune_inventory().unwrap().changed);
    let origin = ulmdb_storage::read_current(&dir.0).unwrap().generation;
    for n in 18..21u64 {
        let mut w = env.begin_write().unwrap();
        w.put(b"kv", &n.to_be_bytes(), &[n as u8; 12_000]).unwrap();
        w.commit().unwrap();
    }
    let manifest = ulmdb_storage::read_current(&dir.0).unwrap();
    assert!(manifest.metadata_origins().contains(&origin));
    assert_ne!(origin, manifest.generation);
    assert!(!manifest.segments.iter().any(|e| e.cohort == origin));
    let original = dir.0.join("cohorts").join(format!("{origin:032x}.cohort"));
    let archive = dir.0.join("commits").join(format!("{origin:032x}.history"));
    let coordinator =
        ulmdb_storage::coordination::Coordination::open(&dir.0, manifest.uuid, Default::default())
            .unwrap();
    let gate = coordinator.try_maintenance().unwrap();
    assert!(
        gate.archive_cohort_history(origin, 16 << 20, 100_000)
            .unwrap()
            .created
    );
    let original_bytes = fs::read(&original).unwrap();
    fs::remove_file(&original).unwrap();
    drop(gate);
    // Fresh storage decoding proves the archive actually supplies live NodeRefs.
    let fresh = ulmdb_storage::read_current(&dir.0).unwrap();
    assert_eq!(fresh.metadata_origins(), manifest.metadata_origins());
    (dir, env, original, archive, original_bytes)
}

#[test]
fn archived_metadata_origin_verify_export_restore_and_dependency_validation() {
    let (dir, env, original, archive, _) = archived_origin();
    assert!(!original.exists());
    let summary = verify(&env).unwrap();
    let export = dir.0.join("export");
    let exported = super::export(&env, &export).unwrap();
    assert_eq!(summary, exported);
    let relative = archive.strip_prefix(&dir.0).unwrap();
    assert_eq!(
        fs::read(&archive).unwrap(),
        fs::read(export.join(relative)).unwrap()
    );
    assert!(!export.join(original.strip_prefix(&dir.0).unwrap()).exists());
    let copy = Environment::open(&export).unwrap();
    assert_eq!(
        verify(&copy).unwrap().physical_bytes,
        summary.physical_bytes
    );
    for n in 0..21u64 {
        assert_eq!(
            copy.begin_read().get(b"kv", &n.to_be_bytes()).unwrap(),
            Some([n as u8; 12_000].as_slice())
        );
    }
    drop(copy);
    let restored = dir.0.join("restored");
    super::restore(&export, &restored).unwrap();
    verify(&Environment::open(&restored).unwrap()).unwrap();

    // Present invalid originals must never fall through to a valid archive.
    fs::write(&original, b"invalid present cohort").unwrap();
    assert!(verify(&env).is_err());
    fs::remove_file(&original).unwrap();
    // An absent or malformed archive remains a hard dependency failure.
    let bytes = fs::read(&archive).unwrap();
    fs::remove_file(&archive).unwrap();
    assert!(verify(&env).is_err());
    fs::write(&archive, b"invalid archive").unwrap();
    assert!(verify(&env).is_err());
    fs::write(&archive, bytes).unwrap();
    verify(&env).unwrap();
}

#[test]
fn controlled_export_retains_archived_metadata_dependencies() {
    let (dir, env, original, archive, _) = archived_origin();
    let target = dir.0.join("controlled-export");
    let control = super::BackupControl::default();
    let summary = super::export_with_control(&env, &target, &control).unwrap();
    assert!(!original.exists());
    let relative = archive.strip_prefix(&dir.0).unwrap();
    assert_eq!(
        fs::read(&archive).unwrap(),
        fs::read(target.join(relative)).unwrap()
    );
    let copy = Environment::open(&target).unwrap();
    assert_eq!(super::verify(&copy).unwrap(), summary);
    for n in 0..21u64 {
        assert_eq!(
            copy.begin_read().get(b"kv", &n.to_be_bytes()).unwrap(),
            Some([n as u8; 12_000].as_slice())
        );
    }
    assert!(control.progress().units != 0);
}

#[test]
fn opened_metadata_origin_copy_survives_archival_without_holding_capture() {
    let (dir, env, original, archive, bytes) = archived_origin();
    fs::write(&original, &bytes).unwrap();
    let origin = u128::from_str_radix(original.file_stem().unwrap().to_str().unwrap(), 16).unwrap();
    let (path, input, kind) = metadata_origin_source(&original, origin).unwrap();
    assert_eq!(path, original);
    // Model maintenance's atomic archive-before-payload-retirement transition
    // after source admission. No Capture guard prevents that progress.
    assert!(archive.exists());
    fs::remove_file(&original).unwrap();
    let target = dir.0.join("opened-origin-copy");
    let length = input.metadata().unwrap().len();
    copy_physical_input(input, &target, &PhysicalFile { kind, prefix: None }, length).unwrap();
    assert_eq!(fs::read(&target).unwrap(), bytes);
    let (path, _, kind) = metadata_origin_source(&original, origin).unwrap();
    assert_eq!(path, archive);
    assert!(matches!(kind, PhysicalKind::MetadataArchive));
    let read = env.begin_read();
    let entry = &read.file_manifest().unwrap().segments[0];
    assert!(
        physical_length(
            &original,
            &PhysicalFile {
                kind: PhysicalKind::Cohort,
                prefix: Some(entry),
            }
        )
        .is_err(),
        "a metadata archive must never stand in for a data payload"
    );
}

#[test]
fn archived_metadata_origin_independent_export_preserves_checkpoint_and_archive() {
    let (dir, env, original, archive, _) = archived_origin();
    let read = ReadTxn::open_coordinated_snapshot(&dir.0).unwrap();
    let checkpoint = read.file_checkpoint().unwrap();
    let target = dir.0.join("independent-export");
    let expected = verify(&env).unwrap();
    let report = export_independent_snapshot(&dir.0, &read, &target).unwrap();
    assert_eq!(report, expected);
    assert_eq!(
        fs::read(target.join(archive.strip_prefix(&dir.0).unwrap())).unwrap(),
        fs::read(&archive).unwrap()
    );
    assert!(!target.join(original.strip_prefix(&dir.0).unwrap()).exists());
    let copy = Environment::open(&target).unwrap();
    assert_eq!(copy.file_checkpoint().unwrap(), checkpoint);
    assert_eq!(verify(&copy).unwrap(), report);
    for n in 0..21u64 {
        assert_eq!(
            copy.begin_read().get(b"kv", &n.to_be_bytes()).unwrap(),
            Some([n as u8; 12_000].as_slice())
        );
    }

    // Even with a previously admitted read and a valid archive, an existing
    // damaged original is a hard failure before target creation/selection.
    fs::write(&original, b"invalid present cohort").unwrap();
    let rejected = dir.0.join("independent-rejected");
    assert!(export_independent_snapshot(&dir.0, &read, &rejected).is_err());
    assert!(!rejected.exists());
}

#[test]
fn archived_metadata_origin_pair_copy_preserves_both_roots_and_union_assets() {
    let (dir, _env, original, archive, original_bytes) = archived_origin();
    let capture = crate::OwnedFileCapture::current_for_copy(
        &dir.0,
        ulmdb_storage::coordination::Coordination::capture_directory(&dir.0, Default::default())
            .unwrap(),
    ).unwrap();
    let source = capture.into_copy_pair().unwrap()
        .admit(crate::ResidencyPolicy::Demand).unwrap();
    let selected = source.selected().file_checkpoint().unwrap();
    let predecessor = source.predecessor().unwrap().file_checkpoint().unwrap();
    let target = dir.0.join("pair-export");
    let report = export_file_pair(&source, &target).unwrap();
    assert_eq!(report.source_selected_checkpoint, selected);
    assert_eq!(report.selected_checkpoint, selected);
    assert_eq!(report.predecessor_checkpoint, Some(predecessor));
    assert_eq!(report.normalization, None);
    assert_eq!(
        fs::read(target.join(archive.strip_prefix(&dir.0).unwrap())).unwrap(),
        fs::read(&archive).unwrap()
    );
    assert!(!target.join(original.strip_prefix(&dir.0).unwrap()).exists());
    let copy = Environment::open(&target).unwrap();
    assert_eq!(copy.file_checkpoint().unwrap(), selected);
    assert_eq!(verify(&copy).unwrap(), report.selected);
    for n in 0..21u64 {
        assert_eq!(
            copy.begin_read().get(b"kv", &n.to_be_bytes()).unwrap(),
            Some([n as u8; 12_000].as_slice())
        );
    }
    let old = ReadTxn::open_previous_coordinated_snapshot(
        &target, crate::ResidencyPolicy::Demand,
    ).unwrap();
    assert_eq!(old.file_checkpoint().unwrap(), predecessor);
    for n in 0..20u64 {
        assert_eq!(
            old.get(b"kv", &n.to_be_bytes()).unwrap(),
            Some([n as u8; 12_000].as_slice())
        );
    }
    assert_eq!(old.get(b"kv", &20u64.to_be_bytes()).unwrap(), None);

    // Present originals resolve to Cohort for both metadata and data roles;
    // retain the complete original rather than silently preferring its archive.
    fs::write(&original, &original_bytes).unwrap();
    let present_target = dir.0.join("pair-original-export");
    export_file_pair(&source, &present_target).unwrap();
    assert_eq!(
        fs::read(present_target.join(original.strip_prefix(&dir.0).unwrap())).unwrap(),
        original_bytes
    );
    assert!(!present_target.join(archive.strip_prefix(&dir.0).unwrap()).exists());
    verify(&Environment::open(&present_target).unwrap()).unwrap();

    fs::write(&original, b"invalid present cohort").unwrap();
    let rejected = dir.0.join("pair-rejected");
    assert!(export_file_pair(&source, &rejected).is_err());
    assert!(!rejected.exists());
}
