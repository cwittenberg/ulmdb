use super::*;
struct Directory(PathBuf);
impl Directory {
    fn new() -> Self {
        let path = std::env::temp_dir().join(format!(
            "ulmdb-cohort-publication-{}",
            crate::new_identity().unwrap()
        ));
        crate::create_layout(&path).unwrap();
        Self(path)
    }
}
impl Drop for Directory {
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.0);
    }
}
fn overwrite(path: &Path, bytes: &[u8]) {
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        fs::set_permissions(path, fs::Permissions::from_mode(0o600)).unwrap();
    }
    fs::write(path, bytes).unwrap();
}
fn initial() -> Manifest {
    let mut manifest = Manifest::empty().unwrap();
    manifest.format_minor = 3;
    manifest
}

#[test]
fn indexed_typed_records_reject_substitution_and_selected_root_fallback() {
    let dir = Directory::new();
    let mut manifest = Manifest::empty().unwrap();
    manifest.format_minor = 4;
    manifest.prepare_indexed(None).unwrap();
    publish(&dir.0, &manifest).unwrap();
    let checkpoint = checkpoint_bytes(&manifest).unwrap();
    assert_eq!(&checkpoint[..8], b"ULMDBT04");
    assert_eq!(
        read_checkpoint(&dir.0, &checkpoint).unwrap().format_minor,
        4
    );
    assert_eq!(
        read_current(&dir.0).unwrap().generation,
        manifest.generation
    );
    for wrong in [b"ULMDBCUR", b"ULMDBC03"] {
        let mut substituted = checkpoint;
        substituted[..8].copy_from_slice(wrong);
        let checksum = crc(&substituted[..60]);
        substituted[60..].copy_from_slice(&checksum.to_le_bytes());
        assert!(read_checkpoint(&dir.0, &substituted).is_err());
    }
    let empty_stage = Directory::new();
    // Fallback is only for referenced old metadata. It cannot select a missing
    // staged root from an otherwise complete existing environment.
    assert!(read_checkpoint_with_fallback(&empty_stage.0, &dir.0, &checkpoint).is_err());
    assert!(decode_current(&checkpoint).is_err());
    let encoded = bundle(checkpoint, None);
    for index in 0..encoded.len() {
        for bit in 0..8 {
            let mut damaged = encoded;
            damaged[index] ^= 1 << bit;
            assert!(decode_current(&damaged).is_err(), "byte={index} bit={bit}");
        }
    }
}
fn next(old: &Manifest) -> Manifest {
    let mut new = old.clone();
    new.previous = old.generation;
    new.parent = old.commit;
    new.generation = crate::new_identity().unwrap();
    new.commit = new.generation;
    new.sequence += 1;
    new
}
#[test]
fn exact_bundle_and_every_single_bit_damage_rejects_without_fallback() {
    let manifest = initial();
    let cur = record(&manifest, [7; 32]);
    for previous in [None, Some(pointer(7, [9; 32]))] {
        let encoded = bundle(cur, previous);
        let decoded = decode_current(&encoded).unwrap();
        assert_eq!(decoded.current, cur);
        assert_eq!(decoded.previous, previous);
        for length in 0..encoded.len() {
            assert!(decode_current(&encoded[..length]).is_err());
        }
        for index in 0..encoded.len() {
            for bit in 0..8 {
                let mut damaged = encoded;
                damaged[index] ^= 1 << bit;
                assert!(decode_current(&damaged).is_err(), "byte={index} bit={bit}");
            }
        }
    }
    assert!(decode_current(&cur).is_err());
    assert!(decode_current(&pointer(7, [9; 32])).is_ok());
}
#[test]
fn legacy_transition_and_bundled_predecessor_ignore_stale_external_previous() {
    let dir = Directory::new();
    let legacy = Manifest::empty().unwrap();
    publish(&dir.0, &legacy).unwrap();
    let mut new = next(&legacy);
    new.format_minor = 3;
    publish(&dir.0, &new).unwrap();
    assert_eq!(fs::read(dir.0.join("CURRENT")).unwrap().len(), BUNDLE_BYTES);
    assert_eq!(read_current(&dir.0).unwrap().generation, new.generation);
    assert_eq!(
        read_previous(&dir.0).unwrap().unwrap().generation,
        legacy.generation
    );
    fs::write(dir.0.join("PREVIOUS"), b"stale and corrupt").unwrap();
    assert_eq!(
        read_previous(&dir.0).unwrap().unwrap().generation,
        legacy.generation
    );
    let third = next(&new);
    let prepared = prepare_cohort(&dir.0, &third, None).unwrap();
    assert_eq!(read_current(&dir.0).unwrap().generation, new.generation);
    prepared.publish().unwrap();
    assert_eq!(
        read_previous(&dir.0).unwrap().unwrap().generation,
        new.generation
    );
    let mut bytes = fs::read(dir.0.join("CURRENT")).unwrap();
    bytes[80..].copy_from_slice(&checkpoint_bytes(&legacy).unwrap());
    fs::write(dir.0.join("CURRENT"), bytes).unwrap();
    assert!(read_current(&dir.0).is_err());
    assert!(read_previous(&dir.0).is_err());
}
#[test]
fn explicit_bootstrap_preserves_identity_without_inventing_history() {
    let dir = Directory::new();
    let mut manifest = initial();
    manifest.sequence = 91;
    manifest.previous = 12;
    manifest.parent = 13;
    install_snapshot(&dir.0, &manifest).unwrap();
    assert_eq!(read_current(&dir.0).unwrap().previous, 12);
    assert_eq!(read_current(&dir.0).unwrap().sequence, 91);
    assert!(read_previous(&dir.0).unwrap().is_none());
    let before = fs::read(dir.0.join("CURRENT")).unwrap();
    assert!(install_snapshot(&dir.0, &manifest).is_err());
    assert_eq!(fs::read(dir.0.join("CURRENT")).unwrap(), before);
}
#[test]
fn corrupt_selected_cohort_never_uses_legacy_or_previous_root() {
    let dir = Directory::new();
    let old = initial();
    publish(&dir.0, &old).unwrap();
    let new = next(&old);
    publish(&dir.0, &new).unwrap();
    let path = cohort::path(&dir.0, new.generation);
    let mut bytes = fs::read(&path).unwrap();
    bytes[20] ^= 1;
    overwrite(&path, &bytes);
    fs::write(
        dir.0
            .join("commits")
            .join(format!("{:032x}.root", new.generation)),
        new.encode().unwrap(),
    )
    .unwrap();
    assert!(read_current(&dir.0).is_err());
    assert!(read_previous(&dir.0).is_err());
    assert_eq!(
        read_checkpoint(&dir.0, &checkpoint_bytes(&old).unwrap())
            .unwrap()
            .generation,
        old.generation
    );
    let legacy_typed = pointer(old.generation, Sha256::digest(old.encode().unwrap()).into());
    assert!(read_checkpoint(&dir.0, &legacy_typed).is_err());
}
#[test]
fn duplicate_generation_and_malformed_footer_never_overwrite_or_activate() {
    let dir = Directory::new();
    let manifest = initial();
    publish(&dir.0, &manifest).unwrap();
    let path = cohort::path(&dir.0, manifest.generation);
    let original = fs::read(&path).unwrap();
    assert!(prepare_cohort(&dir.0, &manifest, None).is_err());
    assert_eq!(fs::read(&path).unwrap(), original);
    for offset in [
        original.len() - 32,
        original.len() - 24,
        original.len() - 16,
        original.len() - 8,
    ] {
        let mut damaged = original.clone();
        damaged[offset] ^= 1;
        overwrite(&path, &damaged);
        assert!(read_current(&dir.0).is_err());
    }
    overwrite(&path, &original);
    OpenOptions::new()
        .write(true)
        .open(&path)
        .unwrap()
        .set_len(crate::MAX_COHORT_BYTES + 1)
        .unwrap();
    assert!(read_current(&dir.0).is_err());
}

#[test]
fn physical_generation_reuse_across_storage_namespaces_is_rejected() {
    let dir = Directory::new();
    let legacy = Manifest::empty().unwrap();
    publish(&dir.0, &legacy).unwrap();
    let mut cohort = next(&legacy);
    cohort.format_minor = 3;
    publish(&dir.0, &cohort).unwrap();
    let current = fs::read(dir.0.join("CURRENT")).unwrap();
    let mut reused = next(&cohort);
    reused.generation = legacy.generation;
    assert!(prepare_cohort(&dir.0, &reused, None).is_err());
    assert!(!crate::descriptor_path(&dir.0, &reused).exists());
    let mut reversed = next(&cohort);
    reversed.format_minor = 2;
    reversed.generation = cohort.generation;
    assert!(publish(&dir.0, &reversed).is_err());
    assert!(!crate::descriptor_path(&dir.0, &reversed).exists());
    let mut same = next(&cohort);
    same.generation = same.previous;
    assert!(prepare_cohort(&dir.0, &same, None).is_err());
    assert_eq!(fs::read(dir.0.join("CURRENT")).unwrap(), current);
    assert_eq!(read_current(&dir.0).unwrap().generation, cohort.generation);
}

fn versioned_next(old: &Manifest, minor: u16, add_segment: bool) -> Manifest {
    let mut new = old.clone_metadata();
    new.previous = old.generation;
    new.parent = old.commit;
    new.generation = crate::new_identity().unwrap();
    new.commit = new.generation;
    new.sequence += 1;
    new.format_minor = minor;
    new.segments = old.segments.clone();
    if add_segment {
        let id = new.next_segment;
        new.next_segment += 1;
        new.segments.push(crate::SegmentEntry {
            id,
            length: 4096,
            hash: [id as u8; 32],
            cohort: 0,
        });
    }
    if minor == 4 {
        new.prepare_indexed(Some(old)).unwrap();
    }
    new
}
fn assert_roots_equal_standalone(directory: &Path) {
    let selected = read_current(directory).unwrap();
    let previous = read_previous(directory).unwrap();
    let pair = read_roots(directory).unwrap();
    assert_eq!(pair.0.encode().unwrap(), selected.encode().unwrap());
    assert_eq!(pair.0.metadata_origins(), selected.metadata_origins());
    assert_eq!(
        pair.1.as_ref().map(|m| m.encode().unwrap()),
        previous.as_ref().map(|m| m.encode().unwrap())
    );
    assert_eq!(
        pair.1.as_ref().map(Manifest::metadata_origins),
        previous.as_ref().map(Manifest::metadata_origins)
    );
}
#[test]
fn combined_roots_equal_standalone_readers_across_static_mixed_format_trees() {
    let dir = Directory::new();
    let mut manifest = Manifest::empty().unwrap();
    manifest.format_minor = 1;
    publish(&dir.0, &manifest).unwrap();
    assert_roots_equal_standalone(&dir.0);
    for minor in [2, 3, 4, 4, 3, 2, 4, 4] {
        let new = versioned_next(&manifest, minor, true);
        publish(&dir.0, &new).unwrap();
        if minor >= 3 {
            fs::write(dir.0.join("PREVIOUS"), b"unselected stale metadata").unwrap();
        }
        assert_roots_equal_standalone(&dir.0);
        let (selected, previous) = read_roots(&dir.0).unwrap();
        assert_eq!(selected.generation, new.generation);
        assert_eq!(previous.unwrap().generation, manifest.generation);
        manifest = new;
    }
}
#[test]
fn combined_roots_preserve_bootstrap_omission_for_every_supported_format() {
    for minor in [1, 2, 3, 4] {
        let dir = Directory::new();
        let mut manifest = Manifest::empty().unwrap();
        manifest.format_minor = minor;
        manifest.previous = 17;
        manifest.parent = 19;
        manifest.sequence = 23;
        if minor == 4 {
            manifest.prepare_indexed(None).unwrap();
        }
        install_snapshot(&dir.0, &manifest).unwrap();
        assert_roots_equal_standalone(&dir.0);
        let (selected, previous) = read_roots(&dir.0).unwrap();
        assert_eq!(selected.previous, 17);
        assert_eq!(selected.sequence, 23);
        assert!(previous.is_none());
    }
}
#[test]
fn combined_roots_reject_partial_extra_and_unbundled_typed_current_records() {
    for minor in [2, 3, 4] {
        let dir = Directory::new();
        let mut old = Manifest::empty().unwrap();
        old.format_minor = minor;
        if minor == 4 {
            old.prepare_indexed(None).unwrap();
        }
        publish(&dir.0, &old).unwrap();
        let new = versioned_next(&old, minor, false);
        publish(&dir.0, &new).unwrap();
        let path = dir.0.join("CURRENT");
        let original = fs::read(&path).unwrap();
        for length in 0..original.len() {
            fs::write(&path, &original[..length]).unwrap();
            assert!(read_roots(&dir.0).is_err(), "minor={minor} length={length}");
        }
        for extra in [1, 64, 80, 145] {
            let mut bytes = original.clone();
            bytes.extend(std::iter::repeat_n(0, extra));
            fs::write(&path, bytes).unwrap();
            assert!(read_roots(&dir.0).is_err());
        }
        if minor >= 3 {
            fs::write(&path, checkpoint_bytes(&new).unwrap()).unwrap();
            assert!(read_roots(&dir.0).is_err());
        }
        fs::write(&path, original).unwrap();
        assert_roots_equal_standalone(&dir.0);
    }
}
#[test]
fn combined_roots_never_fall_back_from_a_corrupt_selected_descriptor() {
    for minor in [2, 3, 4] {
        let dir = Directory::new();
        let mut old = Manifest::empty().unwrap();
        old.format_minor = minor;
        if minor == 4 {
            old.prepare_indexed(None).unwrap();
        }
        publish(&dir.0, &old).unwrap();
        let new = versioned_next(&old, minor, false);
        publish(&dir.0, &new).unwrap();
        let path = crate::descriptor_path(&dir.0, &new);
        let original = fs::read(&path).unwrap();
        let mut corrupt = original.clone();
        corrupt[20] ^= 1;
        overwrite(&path, &corrupt);
        // An intact predecessor and another namespace never substitute for the
        // exact selected type/path, even with an otherwise valid descriptor.
        let shadow = if minor >= 3 {
            dir.0
                .join("commits")
                .join(format!("{:032x}.root", new.generation))
        } else {
            cohort::path(&dir.0, new.generation)
        };
        fs::write(shadow, new.encode().unwrap()).unwrap();
        assert!(read_roots(&dir.0).is_err());
        assert!(read_current(&dir.0).is_err());
        if minor == 2 {
            assert_eq!(
                read_previous(&dir.0).unwrap().unwrap().generation,
                old.generation
            );
        } else {
            assert!(read_previous(&dir.0).is_err());
        }
        assert_eq!(
            read_checkpoint(&dir.0, &checkpoint_bytes(&old).unwrap())
                .unwrap()
                .generation,
            old.generation
        );
        overwrite(&path, &original);
        assert_roots_equal_standalone(&dir.0);
    }
}

#[test]
fn rollback_selects_fresh_root_without_rotating_and_rejects_changed_head() {
    for minor in [2, 3, 4] {
        let dir = Directory::new();
        let mut initial = Manifest::empty().unwrap();
        initial.format_minor = minor;
        if minor == 4 {
            initial.prepare_indexed(None).unwrap();
        }
        publish(&dir.0, &initial).unwrap();
        let a = versioned_next(&initial, minor, false);
        publish(&dir.0, &a).unwrap();
        let b = versioned_next(&a, minor, false);
        publish(&dir.0, &b).unwrap();
        let bytes_a = fs::read(crate::descriptor_path(&dir.0, &a)).unwrap();
        let bytes_b = fs::read(crate::descriptor_path(&dir.0, &b)).unwrap();
        let ancillary = fs::read(dir.0.join("PREVIOUS")).ok();
        let mut restored = a.clone();
        restored.generation = crate::new_identity().unwrap();
        restored.previous = 0;
        if minor == 4 {
            restored.prepare_indexed(Some(&a)).unwrap();
        }
        let rollback = prepare_rollback(&dir.0, &restored, &checkpoint_bytes(&b).unwrap()).unwrap();
        assert_eq!(read_current(&dir.0).unwrap().generation, b.generation);
        rollback.publish().unwrap();
        let (selected, previous) = read_roots(&dir.0).unwrap();
        assert_eq!(selected.generation, restored.generation);
        assert_eq!(selected.commit, a.commit);
        assert_eq!(selected.sequence, a.sequence);
        assert_eq!(selected.previous, 0);
        assert!(previous.is_none());
        assert!(read_previous(&dir.0).unwrap().is_none());
        assert_eq!(
            fs::read(crate::descriptor_path(&dir.0, &a)).unwrap(),
            bytes_a
        );
        assert_eq!(
            fs::read(crate::descriptor_path(&dir.0, &b)).unwrap(),
            bytes_b
        );
        assert_eq!(fs::read(dir.0.join("PREVIOUS")).ok(), ancillary);
        let mut retry = restored.clone();
        retry.generation = crate::new_identity().unwrap();
        if minor == 4 {
            retry.prepare_indexed(Some(&restored)).unwrap();
        }
        let stale =
            prepare_rollback(&dir.0, &retry, &checkpoint_bytes(&restored).unwrap()).unwrap();
        let later = versioned_next(&restored, minor, false);
        publish(&dir.0, &later).unwrap();
        let before = fs::read(dir.0.join("CURRENT")).unwrap();
        assert!(stale.publish().is_err());
        assert_eq!(fs::read(dir.0.join("CURRENT")).unwrap(), before);
    }
}

#[test]
fn legacy_zero_predecessor_ignores_only_ancillary_history_after_current_validation() {
    let dir = Directory::new();
    let initial = Manifest::empty().unwrap();
    publish(&dir.0, &initial).unwrap();
    for invalid in [b"corrupt".as_slice(), &[0; 65]] {
        fs::write(dir.0.join("PREVIOUS"), invalid).unwrap();
        assert!(read_previous(&dir.0).unwrap().is_none());
        assert!(read_roots(&dir.0).unwrap().1.is_none());
    }
    let valid_current = fs::read(dir.0.join("CURRENT")).unwrap();
    let mut corrupt = valid_current.clone();
    corrupt[28] ^= 1;
    fs::write(dir.0.join("CURRENT"), corrupt).unwrap();
    assert!(read_previous(&dir.0).is_err());
    assert!(read_roots(&dir.0).is_err());
    fs::write(dir.0.join("CURRENT"), valid_current).unwrap();
    let path = crate::descriptor_path(&dir.0, &initial);
    let mut corrupt = fs::read(&path).unwrap();
    corrupt[20] ^= 1;
    overwrite(&path, &corrupt);
    assert!(read_previous(&dir.0).is_err());
    assert!(read_roots(&dir.0).is_err());
}
#[test]
fn combined_roots_validate_previous_without_broadening_standalone_current_validation() {
    for minor in [2, 3, 4] {
        let dir = Directory::new();
        let mut old = Manifest::empty().unwrap();
        old.format_minor = minor;
        if minor == 4 {
            old.prepare_indexed(None).unwrap();
        }
        publish(&dir.0, &old).unwrap();
        let new = versioned_next(&old, minor, false);
        publish(&dir.0, &new).unwrap();
        let path = crate::descriptor_path(&dir.0, &old);
        let original = fs::read(&path).unwrap();
        let mut corrupt = original.clone();
        corrupt[20] ^= 1;
        overwrite(&path, &corrupt);
        assert_eq!(read_current(&dir.0).unwrap().generation, new.generation);
        assert!(read_roots(&dir.0).is_err());
        assert!(read_previous(&dir.0).is_err());
        overwrite(&path, &original);
        if minor == 2 {
            let previous = dir.0.join("PREVIOUS");
            fs::remove_file(&previous).unwrap();
            let pair = read_roots(&dir.0).unwrap();
            assert!(pair.1.is_none());
            for invalid in [vec![], vec![0; 63], vec![0; 65], {
                let mut b = checkpoint_bytes(&old).unwrap().to_vec();
                b.push(0);
                b
            }] {
                fs::write(&previous, invalid).unwrap();
                assert!(read_roots(&dir.0).is_err());
                assert_eq!(read_current(&dir.0).unwrap().generation, new.generation);
            }
        } else {
            // A validly checksummed predecessor pointer with the wrong physical
            // identity rejects before any alternate predecessor can be used.
            let mut bytes = fs::read(dir.0.join("CURRENT")).unwrap();
            bytes[80..144].copy_from_slice(&checkpoint_bytes(&new).unwrap());
            fs::write(dir.0.join("CURRENT"), bytes).unwrap();
            assert!(read_roots(&dir.0).is_err());
            assert!(read_current(&dir.0).is_err());
        }
    }
}
#[test]
fn combined_roots_keep_full_indexed_shared_metadata_validation() {
    let dir = Directory::new();
    let mut base = Manifest::empty().unwrap();
    base.format_minor = 4;
    base.segments = ((1..=3)
        .map(|id| crate::SegmentEntry {
            id,
            length: 4096,
            hash: [id as u8; 32],
            cohort: 0,
        })
        .collect::<Vec<_>>())
    .into();
    base.next_segment = 4;
    base.prepare_indexed(None).unwrap();
    publish(&dir.0, &base).unwrap();
    let previous = versioned_next(&base, 4, true);
    publish(&dir.0, &previous).unwrap();
    let selected = versioned_next(&previous, 4, true);
    publish(&dir.0, &selected).unwrap();
    assert_roots_equal_standalone(&dir.0);
    assert!(selected.metadata_origins().contains(&base.generation));
    let origin = cohort::path(&dir.0, base.generation);
    let mut bytes = fs::read(&origin).unwrap();
    bytes[256 + 48] ^= 1;
    overwrite(&origin, &bytes);
    assert!(read_roots(&dir.0).is_err());
    assert!(read_current(&dir.0).is_err());
}

#[test]
fn checked_roots_preserve_selected_check_error_precedence_over_corrupt_predecessor() {
    for minor in [2, 3, 4] {
        let dir = Directory::new();
        let mut old = Manifest::empty().unwrap();
        old.format_minor = minor;
        if minor == 4 {
            old.prepare_indexed(None).unwrap();
        }
        publish(&dir.0, &old).unwrap();
        let selected = versioned_next(&old, minor, false);
        publish(&dir.0, &selected).unwrap();
        let path = crate::descriptor_path(&dir.0, &old);
        let mut corrupt = fs::read(&path).unwrap();
        corrupt[20] ^= 1;
        overwrite(&path, &corrupt);
        let calls = std::cell::Cell::new(0);
        let result = read_roots_checked(&dir.0, |manifest| {
            calls.set(calls.get() + 1);
            assert_eq!(manifest.generation, selected.generation);
            Err(Error::OutcomeUnknown)
        });
        assert!(matches!(result, Err(Error::OutcomeUnknown)));
        assert_eq!(calls.get(), 1);
        // Accepting the head still requires complete predecessor validation.
        assert!(matches!(
            read_roots_checked(&dir.0, |_| Ok(())),
            Err(Error::Corrupt(_))
        ));
        assert_eq!(
            read_current(&dir.0).unwrap().generation,
            selected.generation
        );
    }
}
#[test]
fn checked_roots_do_not_invoke_callback_before_selected_and_bundle_identity_validation() {
    let dir = Directory::new();
    let old = initial();
    publish(&dir.0, &old).unwrap();
    let selected = versioned_next(&old, 3, false);
    publish(&dir.0, &selected).unwrap();
    let original = fs::read(dir.0.join("CURRENT")).unwrap();
    let mut wrong_previous = original.clone();
    wrong_previous[80..144].copy_from_slice(&checkpoint_bytes(&selected).unwrap());
    fs::write(dir.0.join("CURRENT"), wrong_previous).unwrap();
    let calls = std::cell::Cell::new(0);
    assert!(
        read_roots_checked(&dir.0, |_| {
            calls.set(calls.get() + 1);
            Ok(())
        })
        .is_err()
    );
    assert_eq!(calls.get(), 0);
    fs::write(dir.0.join("CURRENT"), original).unwrap();
    let path = crate::descriptor_path(&dir.0, &selected);
    let mut corrupt = fs::read(&path).unwrap();
    corrupt[20] ^= 1;
    overwrite(&path, &corrupt);
    assert!(
        read_roots_checked(&dir.0, |_| {
            calls.set(calls.get() + 1);
            Ok(())
        })
        .is_err()
    );
    assert_eq!(calls.get(), 0);
}
