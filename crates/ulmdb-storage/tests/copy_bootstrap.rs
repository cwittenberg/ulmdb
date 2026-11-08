#![cfg(target_os = "linux")]
use std::{fs, path::PathBuf};
use ulmdb_storage::{
    BootstrapPairTarget, Durability, Error, Manifest, prepare_publication,
    prepare_publication_with_durability,
};
struct Dir(PathBuf);
impl Dir {
    fn new() -> Self {
        let p = std::env::temp_dir().join(format!(
            "copy-bootstrap-{}",
            ulmdb_storage::new_identity().unwrap()
        ));
        fs::create_dir(&p).unwrap();
        Self(p)
    }
}
impl Drop for Dir {
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.0);
    }
}
fn empty(format: u16) -> Manifest {
    let mut m = Manifest::empty().unwrap();
    m.format_minor = format;
    if format == 4 {
        m.prepare_indexed(None).unwrap();
    }
    m
}
#[test]
fn bootstrap_rejects_foreign_namespace_tokens_and_weak_data_before_selection() {
    for format in [2, 3, 4] {
        let d = Dir::new();
        let a = d.0.join("a");
        let b = d.0.join("b");
        let ta = BootstrapPairTarget::create(&a).unwrap();
        let tb = BootstrapPairTarget::create(&b).unwrap();
        let m = empty(format);
        let wrong = prepare_publication(&b, &m).unwrap();
        assert!(ta.prepare(wrong, None).is_err());
        assert!(!a.join("CURRENT").exists());
        let weak =
            prepare_publication_with_durability(&b, &empty(format), Durability::NoSync).unwrap();
        assert!(tb.prepare(weak, None).is_err());
        assert!(!b.join("CURRENT").exists());
        let existing = d.0.join("existing");
        fs::create_dir(&existing).unwrap();
        assert!(BootstrapPairTarget::create(&existing).is_err());
        assert_eq!(fs::read_dir(existing).unwrap().count(), 0);
    }
}
#[test]
fn bootstrap_rejects_wrong_realm_generation_and_lineage_atomically() {
    for format in [2, 3, 4] {
        for fault in ["realm", "generation", "lineage"] {
            let d = Dir::new();
            let path = d.0.join("target");
            let t = BootstrapPairTarget::create(&path).unwrap();
            let p = empty(format);
            let mut c = p.clone_metadata();
            c.generation = ulmdb_storage::new_identity().unwrap();
            c.sequence = 1;
            c.commit = ulmdb_storage::new_identity().unwrap();
            c.parent = p.commit;
            c.previous = p.generation;
            match fault {
                "realm" => c.uuid = ulmdb_storage::new_identity().unwrap().to_le_bytes(),
                "generation" => c.previous = ulmdb_storage::new_identity().unwrap(),
                _ => c.sequence = 7,
            };
            if format == 4 {
                c.prepare_indexed(None).unwrap();
            }
            let pp = prepare_publication(&path, &p).unwrap();
            let pc = prepare_publication(&path, &c).unwrap();
            assert!(matches!(t.prepare(pc, Some(pp)), Err(Error::Corrupt(_))));
            assert!(!path.join("CURRENT").exists());
            assert!(!path.join("PREVIOUS").exists());
        }
    }
}
#[test]
fn prepared_bootstrap_rechecks_actual_directory_after_namespace_replacement() {
    for format in [2, 3, 4] {
        let d = Dir::new();
        let path = d.0.join("target");
        let moved = d.0.join("moved");
        let t = BootstrapPairTarget::create(&path).unwrap();
        let m = empty(format);
        let p = prepare_publication(&path, &m).unwrap();
        let ready = t.prepare(p, None).unwrap();
        fs::rename(&path, &moved).unwrap();
        fs::create_dir(&path).unwrap();
        assert!(matches!(ready.publish(), Err(Error::Corrupt(_))));
        assert!(!path.join("CURRENT").exists());
        assert!(!moved.join("CURRENT").exists());
    }
}
