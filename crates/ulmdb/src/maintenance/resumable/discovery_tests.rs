use super::*;
use crate::{DatabaseKind, Durability, MaintenancePolicy, PublicationMode};
use sha2::{Digest, Sha256};
use std::path::{Path, PathBuf};

const MODES: [PublicationMode; 3] = [
    PublicationMode::Legacy,
    PublicationMode::Cohort,
    PublicationMode::Indexed,
];

struct Directory(PathBuf);
impl Directory {
    fn new() -> Self {
        Self(std::env::temp_dir().join(format!(
            "ulmdb-publication-selector-{}-{}",
            std::process::id(),
            ulmdb_storage::new_identity().unwrap()
        )))
    }
}
impl Drop for Directory {
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.0);
    }
}

fn seed(mode: PublicationMode) -> (Directory, Environment) {
    let directory = Directory::new();
    let env = Environment::create_with_publication(&directory.0, mode).unwrap();
    let mut write = env.begin_write().unwrap();
    write.create_database(b"kv", DatabaseKind::Kv).unwrap();
    assert_eq!(write.commit().unwrap(), 1);
    (directory, env)
}

fn put(env: &Environment, value: &[u8]) -> u64 {
    let mut write = env.begin_write().unwrap();
    write.put(b"kv", b"key", value).unwrap();
    write.commit().unwrap()
}

fn replace_selector(directory: &Path, bytes: &[u8]) {
    let temporary = directory.join("test-CURRENT");
    fs::write(&temporary, bytes).unwrap();
    fs::rename(temporary, directory.join("CURRENT")).unwrap();
}

fn resign_weak(bytes: &mut [u8]) {
    let checksum = Sha256::digest(&bytes[..144]);
    bytes[144..].copy_from_slice(&checksum);
}

#[test]
fn publication_selector_first_and_second_commits_keep_default_maintenance_usable() {
    for mode in MODES {
        let (directory, env) = seed(mode);
        assert!(!env.poisoned.load(Ordering::Acquire), "{mode:?}");
        let old = env.begin_read();
        assert_eq!(old.sequence(), 1);
        assert_eq!(old.get(b"kv", b"key").unwrap(), None);

        assert_eq!(put(&env, b"second"), 2);
        assert!(!env.poisoned.load(Ordering::Acquire), "{mode:?}");
        assert_eq!(old.get(b"kv", b"key").unwrap(), None);
        assert_eq!(
            env.begin_read().get(b"kv", b"key").unwrap(),
            Some(&b"second"[..])
        );
        drop(env.begin_write().unwrap());

        let raw = fs::read(directory.0.join("CURRENT")).unwrap();
        assert_eq!(
            raw.len(),
            if mode == PublicationMode::Legacy {
                64
            } else {
                144
            }
        );
        let observation = ulmdb_storage::observe_selector(&raw).unwrap();
        assert_eq!(observation.checkpoint(), &env.file_checkpoint().unwrap());
        assert!(!observation.is_weak());
        assert!(!observation.has_data_debt());
        drop(old);
        drop(env);

        let reopened = Environment::open(&directory.0).unwrap();
        assert_eq!(
            reopened.begin_read().get(b"kv", b"key").unwrap(),
            Some(&b"second"[..])
        );
        assert_eq!(put(&reopened, b"reopened"), 3);
        assert!(!reopened.poisoned.load(Ordering::Acquire), "{mode:?}");
    }
}

#[test]
fn publication_selector_weak_commits_defer_then_resume_full_maintenance() {
    for mode in MODES {
        for policy in [Durability::NoMetaSync, Durability::NoSync] {
            let (directory, env) = seed(mode);
            env.set_durability(policy).unwrap();
            assert_eq!(put(&env, b"weak"), 2);
            let held = env.begin_read();
            let held_assets: Vec<_> = std::iter::once(ulmdb_storage::descriptor_path(
                &directory.0,
                held.file_manifest().unwrap(),
            ))
            .chain(
                held.file_manifest()
                    .unwrap()
                    .segments
                    .iter()
                    .map(|entry| ulmdb_storage::segment_path(&directory.0, entry)),
            )
            .collect();
            let reclaimed = env.maintenance_status().unwrap().reclaimed_files;
            assert_eq!(put(&env, b"still weak"), 3);
            let raw = fs::read(directory.0.join("CURRENT")).unwrap();
            assert_eq!(raw.len(), 176);
            let observation = ulmdb_storage::observe_selector(&raw).unwrap();
            assert_eq!(observation.checkpoint(), &env.file_checkpoint().unwrap());
            assert!(observation.is_weak());
            assert_eq!(observation.has_data_debt(), policy == Durability::NoSync);
            assert!(!env.poisoned.load(Ordering::Acquire), "{mode:?} {policy:?}");
            // An idle scheduler may finish without attempting discovery. The
            // safety contract is preservation under weak selection, whether
            // there is work to defer or the maintenance step is a no-op.
            env.automatic_reclaim();
            assert_eq!(env.maintenance_status().unwrap().reclaimed_files, reclaimed);
            assert_eq!(fs::read(directory.0.join("CURRENT")).unwrap(), raw);
            assert!(held_assets.iter().all(|path| path.is_file()));
            assert_eq!(held.get(b"kv", b"key").unwrap(), Some(&b"weak"[..]));

            env.set_durability(Durability::Full).unwrap();
            assert_eq!(put(&env, b"full"), 4);
            assert!(!env.poisoned.load(Ordering::Acquire), "{mode:?} {policy:?}");
            assert_eq!(held.get(b"kv", b"key").unwrap(), Some(&b"weak"[..]));
            assert_eq!(
                env.begin_read().get(b"kv", b"key").unwrap(),
                Some(&b"full"[..])
            );
            let raw = fs::read(directory.0.join("CURRENT")).unwrap();
            let observation = ulmdb_storage::observe_selector(&raw).unwrap();
            assert!(!observation.is_weak());
            assert!(!observation.has_data_debt());
            drop(env.begin_write().unwrap());
            assert!(held_assets.iter().all(|path| path.is_file()));
            drop(held);
            env.reclaim().unwrap();
            assert!(!env.poisoned.load(Ordering::Acquire), "{mode:?} {policy:?}");
            assert_eq!(env.begin_read().sequence(), 4);
            assert_eq!(
                env.begin_read().get(b"kv", b"key").unwrap(),
                Some(&b"full"[..])
            );
        }
    }
}

#[test]
fn publication_selector_observation_rejects_malformed_complete_images() {
    for mode in MODES {
        let (directory, env) = seed(mode);
        let raw = fs::read(directory.0.join("CURRENT")).unwrap();
        let checkpoint_start = if raw.len() == 64 { 0 } else { 16 };
        let mut invalid = Vec::new();
        invalid.push(raw[..raw.len() - 1].to_vec());
        let mut wrong_magic = raw.clone();
        wrong_magic[0] ^= 1;
        invalid.push(wrong_magic);
        let mut wrong_checkpoint = raw.clone();
        wrong_checkpoint[checkpoint_start + 28] ^= 1;
        invalid.push(wrong_checkpoint);
        if raw.len() == 144 {
            invalid.push(raw[16..80].to_vec());
            let mut wrong_previous = raw.clone();
            wrong_previous[80 + 28] ^= 1;
            invalid.push(wrong_previous);
            let mut wrong_flags = raw.clone();
            wrong_flags[12..16].copy_from_slice(&2u32.to_le_bytes());
            invalid.push(wrong_flags);
            let mut hidden_previous = raw.clone();
            hidden_previous[12..16].fill(0);
            invalid.push(hidden_previous);
        }
        env.set_durability(Durability::NoSync).unwrap();
        put(&env, b"weak");
        let weak = fs::read(directory.0.join("CURRENT")).unwrap();
        let mut wrong_checksum = weak.clone();
        wrong_checksum[144] ^= 1;
        invalid.push(wrong_checksum);
        let mut wrong_flags = weak.clone();
        wrong_flags[12..16].copy_from_slice(&7u32.to_le_bytes());
        resign_weak(&mut wrong_flags);
        invalid.push(wrong_flags);
        let mut hidden_previous = weak;
        hidden_previous[12..16].copy_from_slice(&2u32.to_le_bytes());
        resign_weak(&mut hidden_previous);
        invalid.push(hidden_previous);
        for bytes in invalid {
            assert!(
                matches!(
                    ulmdb_storage::observe_selector(&bytes),
                    Err(Error::Corrupt(_))
                ),
                "{mode:?}, {} bytes",
                bytes.len()
            );
        }
    }
}

#[test]
fn publication_selector_discovery_rejects_a_different_valid_selected_checkpoint() {
    for mode in MODES {
        let directory = Directory::new();
        let env = Environment::create_with_publication(&directory.0, mode).unwrap();
        let initial = fs::read(directory.0.join("CURRENT")).unwrap();
        let mut write = env.begin_write().unwrap();
        write.create_database(b"kv", DatabaseKind::Kv).unwrap();
        write.commit().unwrap();
        env.set_maintenance_policy(MaintenancePolicy::Disabled)
            .unwrap();
        let _writer = env.writer.lock().unwrap();
        let current = env.current.load_full();
        let checkpoint = *current.physical_pin.as_ref().unwrap().root().checkpoint();
        let mut gate = env
            .coordination
            .as_ref()
            .unwrap()
            .try_maintenance()
            .unwrap();
        let mut progress = Progress::new(&env, &mut gate, checkpoint).unwrap();
        replace_selector(&directory.0, &initial);
        assert!(ulmdb_storage::observe_selector(&initial).is_ok());
        assert!(matches!(
            progress.advance(&env, &mut gate, &current, Default::default(), &mut 0),
            Err(Error::OutcomeUnknown)
        ));
        assert!(env.poisoned.load(Ordering::Acquire));
    }
}

#[test]
fn publication_selector_discovery_preserves_malformed_selector_poisoning() {
    for mode in MODES {
        let (directory, env) = seed(mode);
        env.set_maintenance_policy(MaintenancePolicy::Disabled)
            .unwrap();
        let _writer = env.writer.lock().unwrap();
        let current = env.current.load_full();
        let checkpoint = *current.physical_pin.as_ref().unwrap().root().checkpoint();
        let mut gate = env
            .coordination
            .as_ref()
            .unwrap()
            .try_maintenance()
            .unwrap();
        let mut progress = Progress::new(&env, &mut gate, checkpoint).unwrap();
        let mut raw = fs::read(directory.0.join("CURRENT")).unwrap();
        let at = if raw.len() == 64 { 28 } else { 16 + 28 };
        raw[at] ^= 1;
        replace_selector(&directory.0, &raw);
        assert!(matches!(
            progress.advance(&env, &mut gate, &current, Default::default(), &mut 0),
            Err(Error::OutcomeUnknown)
        ));
        assert!(env.poisoned.load(Ordering::Acquire));
    }
}

#[test]
fn publication_selector_discovery_defers_a_matching_weak_selector_without_poisoning() {
    for mode in MODES {
        for policy in [Durability::NoMetaSync, Durability::NoSync] {
            let (directory, env) = seed(mode);
            env.set_durability(policy).unwrap();
            put(&env, b"weak");
            env.set_maintenance_policy(MaintenancePolicy::Disabled)
                .unwrap();
            let _writer = env.writer.lock().unwrap();
            let current = env.current.load_full();
            let checkpoint = *current.physical_pin.as_ref().unwrap().root().checkpoint();
            let mut gate = env
                .coordination
                .as_ref()
                .unwrap()
                .try_maintenance()
                .unwrap();
            let mut progress = Progress::new(&env, &mut gate, checkpoint).unwrap();
            assert!(matches!(
                progress.advance(&env, &mut gate, &current, Default::default(), &mut 0),
                Err(Error::Busy)
            ));
            assert!(!env.poisoned.load(Ordering::Acquire));
            assert_eq!(
                progress.roots.selected,
                fs::read(directory.0.join("CURRENT")).unwrap()
            );
        }
    }
}

#[test]
fn publication_selector_raw_bundle_replacement_is_detected_before_unlink() {
    for mode in [PublicationMode::Cohort, PublicationMode::Indexed] {
        let (directory, env) = seed(mode);
        env.set_maintenance_policy(MaintenancePolicy::Disabled)
            .unwrap();
        let _writer = env.writer.lock().unwrap();
        let current = env.current.load_full();
        let checkpoint = *current.physical_pin.as_ref().unwrap().root().checkpoint();
        let mut gate = env
            .coordination
            .as_ref()
            .unwrap()
            .try_maintenance()
            .unwrap();
        let mut progress = Progress::new(&env, &mut gate, checkpoint).unwrap();
        progress
            .advance(&env, &mut gate, &current, Default::default(), &mut 0)
            .unwrap();
        let original = fs::read(directory.0.join("CURRENT")).unwrap();
        assert_eq!(progress.roots.selected, original);
        assert_eq!(original.len(), 144);
        let mut replaced = original.clone();
        replaced[12..16].fill(0);
        replaced[80..144].fill(0);
        assert_eq!(
            ulmdb_storage::observe_selector(&replaced)
                .unwrap()
                .checkpoint(),
            &checkpoint
        );
        replace_selector(&directory.0, &replaced);
        assert!(matches!(
            super::super::verify(
                &env,
                &gate,
                &progress.roots.selected,
                &progress.roots.previous
            ),
            Err(Error::OutcomeUnknown)
        ));
        assert!(env.poisoned.load(Ordering::Acquire));
    }
}
