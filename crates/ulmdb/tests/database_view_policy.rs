//! Cross-branch integration: genuine views and prepare/final durability policies.
#![cfg(target_os = "linux")]
use ulmdb::{CommitIntent, DatabaseKind, Durability as D, Environment, Error, PublicationMode};
const MODES: [PublicationMode; 3] = [
    PublicationMode::Legacy,
    PublicationMode::Cohort,
    PublicationMode::Indexed,
];
const POLICIES: [D; 3] = [D::Full, D::NoMetaSync, D::NoSync];
struct Dir(std::path::PathBuf);
impl Dir {
    fn new() -> Self {
        Self(std::env::temp_dir().join(format!(
            "native-policy-views-{}",
            ulmdb_storage::new_identity().unwrap()
        )))
    }
}
impl Drop for Dir {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.0);
    }
}
fn seed(d: &Dir, mode: PublicationMode) -> Environment {
    let e = Environment::create_with_publication(&d.0, mode).unwrap();
    let mut w = e.begin_write().unwrap();
    w.create_database(b"X", DatabaseKind::Kv).unwrap();
    w.put(b"X", b"k", b"B").unwrap();
    w.commit().unwrap();
    e
}
fn state(d: &Dir) -> ulmdb_storage::FileDurabilityState {
    let (_, capture, pin, state) =
        ulmdb_storage::coordination::Coordination::capture_directory(&d.0, Default::default())
            .unwrap()
            .capture_current_with_durability()
            .unwrap();
    drop(pin);
    drop(capture);
    state
}
#[test]
fn selected_branch_data_and_final_policy_matrix_keep_local_weakness_independent() {
    for mode in MODES {
        for data in POLICIES {
            for metadata in POLICIES {
                for local in POLICIES {
                    let d = Dir::new();
                    let e = seed(&d, mode);
                    let old = e.begin_read();
                    let before = std::fs::read(d.0.join("CURRENT")).unwrap();
                    e.set_durability(data).unwrap();
                    let mut w = e.begin_write().unwrap();
                    w.weaken_durability(local).unwrap();
                    let h = w.open_database(b"X").unwrap();
                    let upper = w.open_database_view(&h).unwrap();
                    let lower = w.open_database_view(&h).unwrap();
                    w.view_put(&upper, b"k", b"upper").unwrap();
                    w.view_put(&upper, b"u", b"loser").unwrap();
                    w.view_put(&lower, b"k", b"lower").unwrap();
                    w.select_database_views(&[&lower]).unwrap();
                    let ready = w.prepare().unwrap();
                    assert_eq!(ready.data_policy(), data.weaken(local));
                    assert_eq!(std::fs::read(d.0.join("CURRENT")).unwrap(), before);
                    e.set_durability(metadata).unwrap();
                    ready.commit().unwrap();
                    let selected = state(&d);
                    let debt = data.weaken(local) == D::NoSync;
                    assert_eq!(selected.has_data_debt(), debt);
                    assert_eq!(
                        selected.is_weak(),
                        debt || metadata.weaken(local) != D::Full
                    );
                    assert_eq!(old.get(b"X", b"k").unwrap(), Some(b"B".as_slice()));
                    let r = e.begin_read();
                    assert_eq!(r.get(b"X", b"k").unwrap(), Some(b"lower".as_slice()));
                    assert_eq!(r.get(b"X", b"u").unwrap(), None);
                }
            }
        }
    }
}
#[test]
fn savepoint_release_abort_restore_policy_provenance_and_branches_together() {
    for mode in MODES {
        let d = Dir::new();
        let e = seed(&d, mode);
        let mut w = e.begin_write().unwrap();
        let h = w.open_database(b"X").unwrap();
        let upper = w.open_database_view(&h).unwrap();
        w.view_put(&upper, b"k", b"parent").unwrap();
        let rejected;
        {
            let mut c = w.savepoint().unwrap();
            c.weaken_durability(D::NoSync).unwrap();
            c.create_database(b"temporary", DatabaseKind::Kv).unwrap();
            let t = c.open_database(b"temporary").unwrap();
            rejected = c.open_database_view(&t).unwrap();
            c.view_put(&upper, b"k", b"aborted").unwrap();
            c.abort().unwrap();
        }
        assert_eq!(w.durability(), D::Full);
        assert!(matches!(
            w.view_get(&rejected, b"k"),
            Err(Error::StaleHandle)
        ));
        assert!(matches!(
            w.open_database(b"temporary"),
            Err(Error::MissingDatabase)
        ));
        assert_eq!(
            w.view_get(&upper, b"k").unwrap(),
            Some(b"parent".as_slice())
        );
        let lower;
        {
            let mut c = w.savepoint().unwrap();
            c.weaken_durability(D::NoMetaSync).unwrap();
            lower = c.open_database_view(&h).unwrap();
            c.view_put(&lower, b"k", b"released").unwrap();
            c.release().unwrap();
        }
        assert_eq!(w.durability(), D::NoMetaSync);
        {
            let s = w.database_read_scope().unwrap();
            let a = s.loan_database_view(&upper).unwrap();
            let b = s.loan_database_view(&lower).unwrap();
            let unloaded = s.open_database_view(&h).unwrap();
            e.set_durability(D::NoSync).unwrap();
            assert_eq!(s.get(&a, b"k").unwrap(), Some(b"parent".as_slice()));
            assert_eq!(s.get(&b, b"k").unwrap(), Some(b"released".as_slice()));
            assert_eq!(s.get(&unloaded, b"k").unwrap(), Some(b"B".as_slice()));
            e.sync().unwrap();
        }
        e.set_durability(D::Full).unwrap();
        w.select_database_views(&[&lower]).unwrap();
        let p = w
            .prepare_with_data_policy(CommitIntent::IfChanged, D::Full)
            .unwrap();
        assert_eq!(p.data_policy(), D::NoMetaSync);
        p.commit_with_metadata_policy(D::NoSync).unwrap();
        assert!(!state(&d).has_data_debt());
        assert!(state(&d).is_weak());
        assert_eq!(
            e.begin_read().get(b"X", b"k").unwrap(),
            Some(b"released".as_slice())
        );
    }
}
#[test]
fn every_policy_preparation_entry_rejects_unselected_dirty_views_before_publication() {
    for mode in MODES {
        let d = Dir::new();
        let e = seed(&d, mode);
        let before = std::fs::read(d.0.join("CURRENT")).unwrap();
        for variant in 0..5 {
            let mut w = e.begin_write().unwrap();
            w.weaken_durability(D::NoSync).unwrap();
            let h = w.open_database(b"X").unwrap();
            let v = w.open_database_view(&h).unwrap();
            w.view_put(&v, b"k", b"unselected").unwrap();
            let failed = match variant {
                0 => w.prepare().is_err(),
                1 => w
                    .prepare_with_data_policy(CommitIntent::ForcePublication, D::NoSync)
                    .is_err(),
                2 => w
                    .prepare_for_commit_with_data_policy(
                        CommitIntent::ForcePublication,
                        D::NoMetaSync,
                    )
                    .is_err(),
                3 => w
                    .prepare_for_commit_with_intent(CommitIntent::ForcePublication)
                    .is_err(),
                _ => w
                    .commit_with_intent(CommitIntent::ForcePublication)
                    .is_err(),
            };
            assert!(failed);
            assert_eq!(std::fs::read(d.0.join("CURRENT")).unwrap(), before);
            assert_eq!(
                e.begin_read().get(b"X", b"k").unwrap(),
                Some(b"B".as_slice())
            );
        }
    }
}
#[test]
fn forced_noop_and_aborted_full_prepare_do_not_erase_persistent_debt() {
    for mode in MODES {
        let d = Dir::new();
        let e = seed(&d, mode);
        e.set_durability(D::NoSync).unwrap();
        let mut w = e.begin_write().unwrap();
        let h = w.open_database(b"X").unwrap();
        let v = w.open_database_view(&h).unwrap();
        w.view_put(&v, b"k", b"weak").unwrap();
        w.select_database_views(&[&v]).unwrap();
        w.commit().unwrap();
        drop(e);
        let e = Environment::open(&d.0).unwrap();
        let old = state(&d);
        assert!(old.has_data_debt());
        let mut w = e.begin_write().unwrap();
        let h = w.open_database(b"X").unwrap();
        let v = w.open_database_view(&h).unwrap();
        w.select_database_views(&[&v]).unwrap();
        let p = w
            .prepare_with_intent(CommitIntent::ForcePublication)
            .unwrap();
        assert_eq!(p.data_policy(), D::Full);
        assert!(p.will_publish());
        p.abort();
        assert_eq!(state(&d), old);
        let mut w = e.begin_write().unwrap();
        let h = w.open_database(b"X").unwrap();
        let v = w.open_database_view(&h).unwrap();
        w.select_database_views(&[&v]).unwrap();
        w.prepare_with_intent(CommitIntent::ForcePublication)
            .unwrap()
            .commit()
            .unwrap();
        assert!(!state(&d).has_data_debt());
        assert!(!state(&d).is_weak());
        assert_eq!(
            e.begin_read().get(b"X", b"k").unwrap(),
            Some(b"weak".as_slice())
        );
    }
}
