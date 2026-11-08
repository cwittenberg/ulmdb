#![cfg(any(target_os = "linux", target_os = "macos", windows))]
use std::{collections::BTreeSet, fs, path::PathBuf};
use ulmdb::{DatabaseKind, Environment, PublicationMode};

struct Dir(PathBuf);
impl Dir {
    fn new() -> Self {
        Self(std::env::temp_dir().join(format!(
            "startup-orphans-{}",
            ulmdb_storage::new_identity().unwrap()
        )))
    }
}
impl Drop for Dir {
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.0);
    }
}
fn assets(d: &Dir) -> BTreeSet<PathBuf> {
    ["segments", "cohorts"]
        .into_iter()
        .flat_map(|name| fs::read_dir(d.0.join(name)).unwrap())
        .map(|p| p.unwrap().path())
        .collect()
}

#[test]
fn fresh_owner_collects_aborted_preparations_without_a_rewrite() {
    for mode in [
        PublicationMode::Legacy,
        PublicationMode::Cohort,
        PublicationMode::Indexed,
    ] {
        let d = Dir::new();
        let e = Environment::create_with_publication(&d.0, mode).unwrap();
        let mut w = e.begin_write().unwrap();
        w.create_database(b"db", DatabaseKind::Kv).unwrap();
        w.put(b"db", b"key", &[1; 12_000]).unwrap();
        w.commit().unwrap();
        let stable = e.file_checkpoint().unwrap();
        let live = assets(&d);
        // These are real sealed but unpublished assets, not fabricated segment
        // identities. No retirement or inventory rewrite requests a scan.
        for n in 2..=4 {
            let mut w = e.begin_write().unwrap();
            w.put(b"db", b"key", &vec![n; 2 << 20]).unwrap();
            w.prepare().unwrap().abort();
            assert_eq!(e.file_checkpoint().unwrap(), stable);
        }
        let abandoned = assets(&d).difference(&live).cloned().collect::<Vec<_>>();
        assert!(!abandoned.is_empty(), "prepare must leave real candidates");
        assert!(
            abandoned
                .iter()
                .all(|p| p.metadata().unwrap().len() > 1 << 20)
        );
        let unknown = d.0.join("segments/operator.keep");
        fs::write(&unknown, b"unrecognized operator file").unwrap();
        // A genuine kernel registry pin outlives the original owner. Cleanup
        // must preserve its exact original data, not rely merely on open FDs.
        let old = ulmdb::ReadTxn::open_coordinated_snapshot(&d.0).unwrap();
        let pinned = old
            .file_manifest()
            .unwrap()
            .segments
            .iter()
            .map(|entry| ulmdb_storage::segment_path(&d.0, entry))
            .collect::<Vec<_>>();
        drop(e);
        let e = Environment::open(&d.0).unwrap();
        for n in 5..=36 {
            let mut w = e.begin_write().unwrap();
            w.put(b"db", b"key", &[n; 12_000]).unwrap();
            w.commit().unwrap();
            assert_eq!(old.get(b"db", b"key").unwrap().unwrap()[0], 1);
            assert!(pinned.iter().all(|p| p.exists()), "pinned assets removed");
            let status = e.maintenance_status().unwrap();
            assert_eq!(status.last_reclaim_error, None, "{status:?}");
            assert!(status.last_resumable_mark_units <= 256, "{status:?}");
            assert_eq!(status.rebuilt_candidates, 0, "no full rewrite: {status:?}");
            if abandoned.iter().all(|p| !p.exists()) {
                break;
            }
        }
        assert!(
            abandoned.iter().all(|p| !p.exists()),
            "{:?}",
            e.maintenance_status().unwrap()
        );
        assert_eq!(fs::read(&unknown).unwrap(), b"unrecognized operator file");
        let selected = e.begin_read().get(b"db", b"key").unwrap().unwrap()[0];
        drop((old, e));
        let reopened = Environment::open(&d.0).unwrap();
        assert_eq!(
            reopened.begin_read().get(b"db", b"key").unwrap().unwrap()[0],
            selected
        );
    }
}

#[test]
fn existing_owner_reschedules_discovery_after_prepared_abort_and_drop() {
    for mode in [PublicationMode::Legacy, PublicationMode::Cohort, PublicationMode::Indexed] {
        let d = Dir::new();
        let e = Environment::create_with_publication(&d.0, mode).unwrap();
        let mut w = e.begin_write().unwrap();
        w.create_database(b"db", DatabaseKind::Kv).unwrap();
        w.put(b"db", b"key", &[1; 12_000]).unwrap();
        w.commit().unwrap();
        let old = e.begin_read();
        // Finish startup's initial scan without reaching the rewrite trigger.
        for n in 2..=41 {
            let mut w = e.begin_write().unwrap();
            w.put(b"db", b"key", &[n; 12_000]).unwrap();
            w.commit().unwrap();
        }
        let baseline = assets(&d);
        let selected = e.file_checkpoint().unwrap();
        for n in 42..=44 {
            let mut w = e.begin_write().unwrap();
            w.put(b"db", b"key", &vec![n; 2 << 20]).unwrap();
            let prepared = w.prepare().unwrap();
            if n == 43 { drop(prepared); } else { prepared.abort(); }
            assert_eq!(e.file_checkpoint().unwrap(), selected);
        }
        let abandoned = assets(&d).difference(&baseline).cloned().collect::<Vec<_>>();
        assert!(!abandoned.is_empty());
        for n in 45..=84 {
            let mut w = e.begin_write().unwrap();
            w.put(b"db", b"key", &[n; 12_000]).unwrap();
            w.commit().unwrap();
            assert_eq!(old.get(b"db", b"key").unwrap().unwrap()[0], 1);
            if abandoned.iter().all(|p| !p.exists()) { break; }
        }
        let status = e.maintenance_status().unwrap();
        assert_eq!(status.rebuilt_candidates, 0, "{status:?}");
        assert_eq!(status.fallback_rewrite_attempts, 0, "{status:?}");
        assert!(abandoned.iter().all(|p| !p.exists()), "{mode:?}: {status:?}");
    }
}
