use super::*;
struct Directory(std::path::PathBuf);
impl Directory {
    fn new() -> Self {
        let path = std::env::temp_dir().join(format!(
            "ulmdb-cohort-data-{}",
            crate::new_identity().unwrap()
        ));
        crate::create_layout(&path).unwrap();
        std::fs::create_dir_all(path.join("cohorts")).unwrap();
        Self(path)
    }
}
impl Drop for Directory {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.0);
    }
}
fn payload_arena(id: u64) -> (Arena, Reference) {
    let mut arena = Arena::new(SegmentTable::default(), id).unwrap();
    let value = arena.put_payload(b"identical prefix data").unwrap();
    (arena, value)
}
#[test]
fn cohort_prefix_matches_legacy_and_is_mapped_without_descriptor_or_footer() {
    let legacy = Directory::new();
    let cohort = Directory::new();
    let (old, value) = payload_arena(1);
    let (legacy_table, legacy_entry) = old.seal(&legacy.0).unwrap();
    let legacy_entry = legacy_entry.unwrap();
    let (new, new_value) = payload_arena(1);
    assert_eq!(value, new_value);
    let mut manifest = crate::Manifest::empty().unwrap();
    let (table, prepared) = new.seal_cohort(&cohort.0, &mut manifest).unwrap();
    assert!(!cohort.0.join("CURRENT").exists());
    assert_eq!(manifest.format_minor, 3);
    assert_eq!(manifest.next_segment, 2);
    assert_eq!(manifest.segments.len(), 1);
    let entry = &manifest.segments[0];
    assert_eq!(entry.cohort, manifest.generation);
    assert_eq!(entry.hash, legacy_entry.hash);
    assert_eq!(entry.length, legacy_entry.length);
    assert_eq!(legacy_entry.cohort, 0);
    let legacy_bytes = std::fs::read(crate::segment_path(&legacy.0, &legacy_entry)).unwrap();
    let cohort_path = crate::segment_path(&cohort.0, entry);
    let physical = std::fs::metadata(&cohort_path).unwrap().len();
    assert!(physical >= entry.length + 224);
    let mapping = crate::mapping::Mapping::open(&cohort.0, entry.clone()).unwrap();
    assert_eq!(mapping.bytes.len(), entry.length as usize);
    assert_eq!(mapping.bytes[..], legacy_bytes);
    assert_eq!(table.payload(value).unwrap(), b"identical prefix data");
    assert!(
        table
            .read(
                Reference {
                    segment: 1,
                    offset: entry.length
                },
                1
            )
            .is_err(),
        "descriptor bytes must not be in the segment map"
    );
    let mut wrong = legacy_bytes.clone();
    *wrong.last_mut().unwrap() ^= 1;
    assert!(crate::mapping::Mapping::open_written(&cohort.0, entry.clone(), &wrong).is_err());
    assert!(
        crate::mapping::Mapping::open_written(
            &cohort.0,
            entry.clone(),
            &legacy_bytes[..legacy_bytes.len() - 1]
        )
        .is_err()
    );
    prepared.publish().unwrap();
    let selected = crate::read_current(&cohort.0).unwrap();
    assert_eq!(selected.generation, manifest.generation);
    let reopened = SegmentTable::load(&cohort.0, &selected.segments).unwrap();
    assert_eq!(reopened.payload(value).unwrap(), b"identical prefix data");
    assert_eq!(
        legacy_table.payload(value).unwrap(),
        b"identical prefix data"
    );
}
#[test]
fn mixed_legacy_and_cohort_inventory_and_empty_prepare_keep_identity_counters() {
    let dir = Directory::new();
    let (source, old) = payload_arena(17);
    let (base, entry) = source.seal(&dir.0).unwrap();
    let entry = entry.unwrap();
    let old_pointer = base.payload(old).unwrap().as_ptr();
    let mut writer = Arena::new(base.clone(), 18).unwrap();
    let new = writer.put_payload(b"new cohort data").unwrap();
    let mut manifest = crate::Manifest::empty().unwrap();
    manifest.next_segment = 18;
    let (table, prepared) = writer.seal_cohort(&dir.0, &mut manifest).unwrap();
    assert_eq!(manifest.segments.len(), 2);
    assert_eq!(manifest.segments[0].cohort, 0);
    assert_eq!(manifest.segments[1].cohort, manifest.generation);
    assert_eq!(manifest.next_segment, 19);
    assert_eq!(table.payload(old).unwrap().as_ptr(), old_pointer);
    assert_eq!(table.payload(new).unwrap(), b"new cohort data");
    assert_eq!(base.payload(old).unwrap(), b"identical prefix data");
    prepared.publish().unwrap();
    let mut next = manifest.clone();
    next.previous = manifest.generation;
    next.generation = crate::new_identity().unwrap();
    let empty = Arena::new(table, 19).unwrap();
    let (table, prepared) = empty.seal_cohort(&dir.0, &mut next).unwrap();
    assert_eq!(next.next_segment, 19);
    assert_eq!(next.segments.len(), 2);
    assert!(!next.segments.iter().any(|e| e.cohort == next.generation));
    prepared.publish().unwrap();
    assert_eq!(
        crate::read_current(&dir.0).unwrap().generation,
        next.generation
    );
    assert_eq!(table.payload(new).unwrap(), b"new cohort data");
    assert_eq!(
        crate::read_previous(&dir.0).unwrap().unwrap().generation,
        manifest.generation
    );
    assert_eq!(entry.cohort, 0);
}
#[test]
fn active_checkpoint_and_zero_generation_reject_before_file_preparation() {
    let dir = Directory::new();
    let (mut arena, _) = payload_arena(1);
    let _checkpoint = arena.checkpoint().unwrap();
    let mut manifest = crate::Manifest::empty().unwrap();
    assert!(matches!(
        arena.seal_cohort(&dir.0, &mut manifest),
        Err(Error::InvalidInput(_))
    ));
    assert_eq!(manifest.format_minor, 2);
    assert!(manifest.segments.is_empty());
    assert!(
        std::fs::read_dir(dir.0.join("cohorts"))
            .unwrap()
            .next()
            .is_none()
    );
    let (arena, _) = payload_arena(1);
    manifest.generation = 0;
    assert!(matches!(
        arena.seal_cohort(&dir.0, &mut manifest),
        Err(Error::InvalidInput(_))
    ));
    assert!(
        std::fs::read_dir(dir.0.join("cohorts"))
            .unwrap()
            .next()
            .is_none()
    );
}
#[test]
fn cohort_mapping_rejects_short_overlong_and_changed_prefixes() {
    let dir = Directory::new();
    let (arena, _) = payload_arena(1);
    let mut manifest = crate::Manifest::empty().unwrap();
    let (table, _prepared) = arena.seal_cohort(&dir.0, &mut manifest).unwrap();
    let entry = manifest.segments[0].clone();
    let path = crate::segment_path(&dir.0, &entry);
    let original = std::fs::read(&path).unwrap();
    drop(table);
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        std::fs::set_permissions(&path, std::fs::Permissions::from_mode(0o600)).unwrap();
    }
    std::fs::write(&path, &original[..entry.length as usize + 223]).unwrap();
    assert!(crate::mapping::Mapping::open(&dir.0, entry.clone()).is_err());
    let file = std::fs::OpenOptions::new().write(true).open(&path).unwrap();
    file.set_len(crate::MAX_COHORT_BYTES + 1).unwrap();
    drop(file);
    assert!(crate::mapping::Mapping::open(&dir.0, entry.clone()).is_err());
    let mut corrupted = original;
    corrupted[4096] ^= 1;
    std::fs::write(&path, &corrupted).unwrap();
    assert!(crate::mapping::Mapping::open(&dir.0, entry).is_err());
}

#[test]
fn sealed_creator_handle_maps_exact_prefix_after_unlink_without_reopening() {
    let dir = Directory::new();
    let (mut arena, value) = payload_arena(41);
    let mut manifest = crate::Manifest::empty().unwrap();
    manifest.format_minor = 3;
    let entry = arena.prepare_segment(manifest.generation).unwrap().unwrap();
    manifest.segments = (vec![entry.clone()]).into();
    manifest.next_segment = 42;
    let sealed = crate::cohort::write_open(&dir.0, &manifest, Some(&arena.bytes)).unwrap();
    std::fs::remove_file(crate::segment_path(&dir.0, &entry)).unwrap();
    assert!(crate::mapping::Mapping::open(&dir.0, entry.clone()).is_err());
    let map = crate::mapping::Mapping::from_written_file(
        &sealed.file,
        entry.clone(),
        &arena.bytes,
        sealed.physical,
    )
    .unwrap();
    assert_eq!(map.bytes.len(), entry.length as usize);
    assert_eq!(&map.bytes[..], &arena.bytes);
    let mut wrong = arena.bytes.clone();
    *wrong.last_mut().unwrap() ^= 1;
    assert!(
        crate::mapping::Mapping::from_written_file(
            &sealed.file,
            entry.clone(),
            &wrong,
            sealed.physical,
        )
        .is_err()
    );
    assert!(
        crate::mapping::Mapping::from_written_file(&sealed.file, entry, &arena.bytes, 0,).is_err()
    );
    let mut table = SegmentTable::default();
    table.insert_mapping(map).unwrap();
    drop(sealed);
    assert_eq!(table.payload(value).unwrap(), b"identical prefix data");
}
