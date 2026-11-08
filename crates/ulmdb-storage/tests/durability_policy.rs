//! Policy receipts and selector authority exercised against actual immutable files.
#![cfg(target_os = "linux")]
use sha2::{Digest, Sha256};
use std::{
    fs,
    path::{Path, PathBuf},
};
use ulmdb_storage::{
    Arena, Durability as D, FileDurabilityState, Manifest, PreparedPublication, SegmentTable,
    WriteStore, checkpoint_bytes,
    coordination::{Coordination, Limits},
    create_layout, new_identity, prepare_publication_with_sealed_data,
    prepare_rollback_with_durability, publish, read_current, read_roots, sync_file_environment,
};

struct Fixture {
    path: PathBuf,
}
impl Fixture {
    fn new(format: u16) -> Self {
        let path =
            std::env::temp_dir().join(format!("ulmdb-policy-{:032x}", new_identity().unwrap()));
        create_layout(&path).unwrap();
        let mut m = Manifest::empty().unwrap();
        m.format_minor = format;
        if format == 4 {
            m.prepare_indexed(None).unwrap();
        }
        publish(&path, &m).unwrap();
        Self { path }
    }
    fn state(&self) -> FileDurabilityState {
        state(&self.path)
    }
}
impl Drop for Fixture {
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.path);
    }
}
fn state(path: &Path) -> FileDurabilityState {
    let (_coordinator, capture, pin, state) =
        Coordination::capture_directory(path, Limits::default())
            .unwrap()
            .capture_current_with_durability()
            .unwrap();
    assert_eq!(pin.root().checkpoint(), state.checkpoint());
    drop(pin);
    drop(capture);
    state
}
fn next(path: &Path, policy: D, payload: &[u8]) -> (Manifest, PreparedPublication) {
    build_from(path, read_current(path).unwrap(), policy, payload)
}
fn build_from(
    path: &Path,
    mut old: Manifest,
    policy: D,
    payload: &[u8],
) -> (Manifest, PreparedPublication) {
    let table = SegmentTable::load(path, &old.segments).unwrap();
    if old.format_minor == 4 {
        old.certify_indexed_table(&table).unwrap();
    }
    let mut m = old.clone();
    m.previous = old.generation;
    m.generation = new_identity().unwrap();
    m.sequence += 1;
    m.parent = old.commit;
    m.commit = m.generation;
    let mut arena = Arena::new(table, old.next_segment).unwrap();
    arena.allocate(payload, 8).unwrap();
    let token = match m.format_minor {
        2 => {
            let (table, entry, receipt) = arena.seal_with_durability(path, policy).unwrap();
            m.segments = table.entries().into();
            m.next_segment = entry.unwrap().id + 1;
            prepare_publication_with_sealed_data(path, &m, receipt).unwrap()
        }
        3 => {
            arena
                .seal_cohort_with_durability(path, &mut m, policy)
                .unwrap()
                .1
        }
        4 => {
            arena
                .seal_indexed_with_durability(path, &mut m, &old, policy)
                .unwrap()
                .1
        }
        _ => panic!("unexpected format"),
    };
    (m, token)
}
fn resign(bytes: &mut [u8]) {
    let hash: [u8; 32] = Sha256::digest(&bytes[..144]).into();
    bytes[144..].copy_from_slice(&hash);
}

#[test]
fn actual_data_and_final_metadata_policies_preserve_debt_independently() {
    for format in [2, 3, 4] {
        for data in [D::Full, D::NoMetaSync, D::NoSync] {
            for metadata in [D::Full, D::NoMetaSync, D::NoSync] {
                let f = Fixture::new(format);
                let old = read_current(&f.path).unwrap();
                let old_bytes = fs::read(f.path.join("CURRENT")).unwrap();
                let (m, token) = next(&f.path, data, b"policy matrix actual data");
                let ready = token.prepare_ready(Some(&f.state())).unwrap();
                assert_eq!(fs::read(f.path.join("CURRENT")).unwrap(), old_bytes);
                let state = ready.publish(metadata).unwrap();
                assert!(state.matches(&checkpoint_bytes(&m).unwrap()));
                assert_eq!(state.has_data_debt(), data == D::NoSync);
                assert_eq!(state.is_weak(), data == D::NoSync || metadata != D::Full);
                assert_eq!(read_current(&f.path).unwrap().sequence, m.sequence);
                let (selected, previous) = read_roots(&f.path).unwrap();
                assert_eq!(selected.generation, m.generation);
                assert_eq!(previous.unwrap().generation, old.generation);
                assert_eq!(f.state(), state);
                let pointer = fs::read(f.path.join("CURRENT")).unwrap();
                assert_eq!(
                    pointer.len(),
                    if state.is_weak() {
                        176
                    } else if format == 2 {
                        64
                    } else {
                        144
                    }
                );
            }
        }
    }
}

#[test]
fn strict_weak_container_preserves_old_bundle_type_boundary() {
    for format in [2, 3, 4] {
        let f = Fixture::new(format);
        let (_, p) = next(&f.path, D::NoSync, b"strict envelope");
        p.prepare_ready(None).unwrap().publish(D::NoSync).unwrap();
        let good = fs::read(f.path.join("CURRENT")).unwrap();
        let mut cases = Vec::new();
        let mut changed = good.clone();
        changed[16] ^= 1;
        cases.push(changed);
        let mut changed = good.clone();
        changed[12..16].copy_from_slice(&7u32.to_le_bytes());
        resign(&mut changed);
        cases.push(changed);
        let mut changed = good.clone();
        changed[12..16].copy_from_slice(&2u32.to_le_bytes());
        resign(&mut changed);
        cases.push(changed);
        let mut changed = good.clone();
        changed[8] = 2;
        resign(&mut changed);
        cases.push(changed);
        let mut changed = good.clone();
        changed.push(0);
        cases.push(changed);
        for bad in cases {
            fs::write(f.path.join("CURRENT"), bad).unwrap();
            assert!(read_current(&f.path).is_err());
            assert!(sync_file_environment(&f.path).is_err());
        }
        fs::write(f.path.join("CURRENT"), &good).unwrap();
        assert!(read_current(&f.path).is_ok());
        if format == 2 {
            let mut invalid = vec![0; 144];
            invalid[..8].copy_from_slice(b"ULMDBPUB");
            invalid[10..12].copy_from_slice(&144u16.to_le_bytes());
            invalid[16..80].copy_from_slice(&good[16..80]);
            fs::write(f.path.join("CURRENT"), invalid).unwrap();
            assert!(read_current(&f.path).is_err());
        }
    }
}

#[test]
fn inherited_debt_is_repaired_before_ready_and_force_sync_preserves_selector() {
    for format in [2, 3, 4] {
        let f = Fixture::new(format);
        let (_, p) = next(&f.path, D::NoSync, b"inherited bytes");
        p.prepare_ready(None).unwrap().publish(D::NoSync).unwrap();
        let weak = fs::read(f.path.join("CURRENT")).unwrap();
        let weak_state = f.state();
        assert!(weak_state.has_data_debt());
        let report = sync_file_environment(&f.path).unwrap();
        assert_eq!(report.selected_checkpoint, *weak_state.checkpoint());
        assert_eq!(fs::read(f.path.join("CURRENT")).unwrap(), weak);
        assert!(f.state().has_data_debt());
        let (m, p) = next(&f.path, D::Full, b"next fully fenced bytes");
        let ready = p.prepare_ready(Some(&weak_state)).unwrap();
        assert_eq!(fs::read(f.path.join("CURRENT")).unwrap(), weak);
        let state = ready.publish(D::Full).unwrap();
        assert!(!state.has_data_debt());
        assert!(!state.is_weak());
        assert_eq!(read_current(&f.path).unwrap().generation, m.generation);
        assert!(sync_file_environment(&f.path).is_ok());
    }
}

#[test]
fn weak_legacy_embedded_history_ignores_unrelated_external_previous() {
    let f = Fixture::new(2);
    let (_, p) = next(&f.path, D::NoMetaSync, b"embedded legacy predecessor");
    p.prepare_ready(None)
        .unwrap()
        .publish(D::NoMetaSync)
        .unwrap();
    fs::write(f.path.join("PREVIOUS"), b"unrelated stale external history").unwrap();
    let roots = read_roots(&f.path).unwrap();
    assert!(roots.1.is_some());
    assert!(sync_file_environment(&f.path).is_ok());
    let (_, p) = next(&f.path, D::Full, b"transition back");
    let s = p
        .prepare_ready(Some(&f.state()))
        .unwrap()
        .publish(D::Full)
        .unwrap();
    assert!(!s.is_weak());
    assert_eq!(fs::metadata(f.path.join("CURRENT")).unwrap().len(), 64);
    assert_eq!(fs::metadata(f.path.join("PREVIOUS")).unwrap().len(), 64);
    assert!(sync_file_environment(&f.path).is_ok());
}

#[test]
fn rollback_carries_debt_without_claiming_user_asset_repair() {
    for format in [2, 3, 4] {
        let f = Fixture::new(format);
        let (a, p) = next(&f.path, D::Full, b"root A");
        p.publish().unwrap();
        let (_, p) = next(&f.path, D::NoSync, b"root B");
        p.prepare_ready(None).unwrap().publish(D::NoSync).unwrap();
        let b = f.state();
        let mut r = a;
        r.previous = 0;
        r.generation = new_identity().unwrap();
        if format == 4 {
            r.prepare_indexed(None).unwrap();
        }
        let ready = prepare_rollback_with_durability(&f.path, &r, b.checkpoint(), D::Full)
            .unwrap()
            .prepare_ready(Some(&b))
            .unwrap();
        let out = ready.publish(D::Full).unwrap();
        assert!(out.is_weak());
        assert!(out.has_data_debt());
        assert!(read_roots(&f.path).unwrap().1.is_none());
        assert!(sync_file_environment(&f.path).is_ok());
    }
}

#[test]
fn stale_or_foreign_state_and_changed_selection_are_rejected_without_publication() {
    for format in [2, 3, 4] {
        let f = Fixture::new(format);
        let other = Fixture::new(format);
        let (_, p) = next(&f.path, D::Full, b"foreign state");
        assert!(p.prepare_ready(Some(&other.state())).is_err());
        let before = fs::read(f.path.join("CURRENT")).unwrap();
        let (_, p) = next(&f.path, D::Full, b"ready C");
        let ready = p.prepare_ready(Some(&f.state())).unwrap();
        let (_, competing) = next(&f.path, D::Full, b"competing D");
        competing.publish().unwrap();
        let selected = fs::read(f.path.join("CURRENT")).unwrap();
        assert_ne!(before, selected);
        assert!(ready.publish(D::Full).is_err());
        assert_eq!(fs::read(f.path.join("CURRENT")).unwrap(), selected);
    }
}

#[test]
fn full_fresh_bootstrap_keeps_original_file_classes_without_coordination_records() {
    for format in [2, 3, 4] {
        let f = Fixture::new(format);
        for name in ["capture", "registry", "pins"] {
            assert!(!f.path.join(".ulmdb-coordination").join(name).exists());
        }
        assert_eq!(
            fs::metadata(f.path.join("CURRENT")).unwrap().len(),
            if format == 2 { 64 } else { 144 }
        );
    }
}

#[test]
fn receipt_rejects_foreign_directory_and_modified_new_segment_inventory() {
    let f = Fixture::new(2);
    let other = Fixture::new(2);
    let mut arena = Arena::new(SegmentTable::default(), 1).unwrap();
    arena.allocate(b"receipt identity", 8).unwrap();
    let (table, _, receipt) = arena.seal_with_durability(&f.path, D::Full).unwrap();
    let mut m = read_current(&f.path).unwrap();
    m.segments = table.entries().into();
    assert!(prepare_publication_with_sealed_data(&other.path, &m, receipt).is_err());
    let mut arena = Arena::new(SegmentTable::default(), 1).unwrap();
    arena.allocate(b"receipt identity", 8).unwrap();
    let (_, _, receipt) = arena.seal_with_durability(&f.path, D::Full).unwrap();
    m.segments = Default::default();
    assert!(prepare_publication_with_sealed_data(&f.path, &m, receipt).is_err());
}

#[test]
fn strong_no_debt_readiness_uses_cached_facts_without_capture_or_descriptor_reread() {
    for format in [2, 3, 4] {
        let f = Fixture::new(format);
        let s = f.state();
        let coordinator = Coordination::open(
            &f.path,
            read_current(&f.path).unwrap().uuid,
            Limits::default(),
        )
        .unwrap();
        let _maintenance = coordinator.try_maintenance().unwrap();
        let (m, p) = next(&f.path, D::Full, b"ready under held maintenance");
        let descriptor = ulmdb_storage::descriptor_path(&f.path, &m);
        let hidden = f.path.join("private-hidden-descriptor");
        fs::rename(&descriptor, &hidden).unwrap();
        let ready = p.prepare_ready(Some(&s)).unwrap();
        fs::rename(hidden, descriptor).unwrap();
        ready.publish(D::Full).unwrap();
    }
}

#[test]
fn actual_previous_token_fences_b_but_retains_a_as_logical_predecessor() {
    for format in [2, 3, 4] {
        let f = Fixture::new(format);
        let (a, p) = next(&f.path, D::Full, b"previous A");
        p.publish().unwrap();
        let (b, p) = next(&f.path, D::NoSync, b"displaced B");
        p.prepare_ready(None).unwrap().publish(D::NoSync).unwrap();
        let selection = ulmdb_storage::capture_previous_selection(&f.path)
            .unwrap()
            .unwrap();
        assert_eq!(selection.selected().generation, b.generation);
        assert_eq!(selection.previous().generation, a.generation);
        assert!(selection.selected_durability().has_data_debt());
        let (c, p) = build_from(&f.path, a.clone(), D::Full, b"new C from A");
        let special = ulmdb_storage::prepare_previous_publication(&selection, &c, p).unwrap();
        let ready = special
            .prepare_ready(Some(selection.selected_durability()))
            .unwrap();
        assert_eq!(read_current(&f.path).unwrap().generation, b.generation);
        let state = ready.publish(D::Full).unwrap();
        assert!(!state.has_data_debt());
        assert!(!state.is_weak());
        let roots = read_roots(&f.path).unwrap();
        assert_eq!(roots.0.generation, c.generation);
        assert_eq!(roots.1.unwrap().generation, a.generation);
        assert!(sync_file_environment(&f.path).is_ok());
    }
}

#[test]
fn genuine_legacy_to_cohort_to_indexed_transitions_preserve_typed_weak_history() {
    let f = Fixture::new(2);
    let (a, p) = next(&f.path, D::Full, b"legacy data");
    p.publish().unwrap();
    let mut b = a.clone();
    b.previous = a.generation;
    b.generation = new_identity().unwrap();
    b.sequence += 1;
    b.parent = a.commit;
    b.commit = b.generation;
    let table = SegmentTable::load(&f.path, &a.segments).unwrap();
    let mut arena = Arena::new(table, a.next_segment).unwrap();
    arena.allocate(b"cohort upgrade", 8).unwrap();
    let (_, p) = arena
        .seal_cohort_with_durability(&f.path, &mut b, D::NoSync)
        .unwrap();
    let state = p.prepare_ready(None).unwrap().publish(D::NoSync).unwrap();
    assert!(state.has_data_debt());
    assert_eq!(read_roots(&f.path).unwrap().1.unwrap().format_minor, 2);
    assert!(sync_file_environment(&f.path).is_ok());
    let table = SegmentTable::load(&f.path, &b.segments).unwrap();
    let mut c = b.clone();
    c.previous = b.generation;
    c.generation = new_identity().unwrap();
    c.sequence += 1;
    c.parent = b.commit;
    c.commit = c.generation;
    let mut arena = Arena::new(table, b.next_segment).unwrap();
    arena.allocate(b"indexed upgrade", 8).unwrap();
    let (_, p) = arena
        .seal_indexed_with_durability(&f.path, &mut c, &b, D::Full)
        .unwrap();
    p.prepare_ready(Some(&state))
        .unwrap()
        .publish(D::Full)
        .unwrap();
    assert_eq!(read_current(&f.path).unwrap().format_minor, 4);
    assert_eq!(read_roots(&f.path).unwrap().1.unwrap().format_minor, 3);
    assert!(sync_file_environment(&f.path).is_ok());
}

#[test]
fn ready_and_sealed_receipt_reject_a_replaced_same_spelling_namespace() {
    let f = Fixture::new(2);
    let state = f.state();
    let (_, p) = next(&f.path, D::Full, b"before directory replacement");
    let ready = p.prepare_ready(Some(&state)).unwrap();
    let mut arena = Arena::new(SegmentTable::default(), 1).unwrap();
    arena.allocate(b"namespace receipt", 8).unwrap();
    let (table, _, sealed) = arena.seal_with_durability(&f.path, D::Full).unwrap();
    let mut manifest = read_current(&f.path).unwrap();
    manifest.segments = table.entries().into();
    let displaced = f.path.with_extension("retained-original");
    fs::rename(&f.path, &displaced).unwrap();
    create_layout(&f.path).unwrap();
    let fresh = Manifest::empty().unwrap();
    publish(&f.path, &fresh).unwrap();
    let bytes = fs::read(f.path.join("CURRENT")).unwrap();
    assert!(ready.publish(D::Full).is_err());
    assert!(prepare_publication_with_sealed_data(&f.path, &manifest, sealed).is_err());
    assert_eq!(fs::read(f.path.join("CURRENT")).unwrap(), bytes);
    fs::remove_dir_all(&f.path).unwrap();
    fs::rename(displaced, &f.path).unwrap();
}
