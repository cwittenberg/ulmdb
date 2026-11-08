use super::*;
use crate::{
    WriteStore,
    resident::{Config, Pool, wal_codec_v2},
};
use sha2::{Digest, Sha256};
fn h(j: &Journal) -> Record {
    let s = j.state();
    let b = j.boot();
    Record {
        uuid: b.uuid,
        epoch: b.epoch,
        lsn: s.selected().lsn + 1,
        sequence: s.selected().sequence + 1,
        commit: s.commit() + 1,
        parent_commit: s.commit(),
        parent: s.selected(),
        root: crate::Reference::NULL,
        next_database: 1,
        kind: wal_codec_v2::Kind::Write,
        allocations: Vec::new(),
        retired: Vec::new(),
    }
}
fn digest(p: &Path) -> [u8; 32] {
    Sha256::digest(std::fs::read(p).unwrap()).into()
}
#[test]
fn borrowed_callback_and_canonical_rejection_restore_frame_workspace_without_io() {
    for root in [std::env::temp_dir(), std::path::PathBuf::from("/dev/shm")] {
        let path = root.join(format!(
            "resident-borrowed-buffer-{:032x}",
            crate::new_identity().unwrap()
        ));
        let boot = Boot {
            uuid: [9; 16],
            epoch: 1,
            commit: 8,
            kv_pages: 4,
            tiny_pages: 0,
        };
        let mut j = Journal::create(&path, boot, 1 << 20).unwrap();
        let pool = Pool::create(Config {
            kv_pages: 4,
            tiny_pages: 0,
            max_reserved_bytes: 1 << 20,
            max_metadata_bytes: 16 << 20,
            max_views: 32,
        })
        .unwrap();
        let mut a = pool.begin(pool.empty().unwrap()).unwrap();
        a.allocate(&[7; 4096], 4096).unwrap();
        let candidate = a.freeze().unwrap();
        let mut workspace = wal_codec_v2::RecipeWorkspace::default();
        let before = digest(&path);
        let mut b = wal_codec_v2::RecipeBuilder::new(h(&j), &candidate, &mut workspace).unwrap();
        b.literal(0, 0).unwrap();
        let error = j.commit_borrowed(b.finish(), |_, _| Err(Error::InvalidInput("reject graph")));
        assert!(matches!(error, Err(Error::InvalidInput(_))));
        assert_eq!(j.state().selected().lsn, 0);
        assert_eq!(digest(&path), before);
        let retained = j.retained_encode_bytes();
        assert!(retained > 0 && retained <= wal_codec_v2::MAX_RECORD_BYTES);
        let mut invalid = h(&j);
        invalid.uuid[0] ^= 1;
        let mut b = wal_codec_v2::RecipeBuilder::new(invalid, &candidate, &mut workspace).unwrap();
        b.literal(0, 0).unwrap();
        assert!(
            j.commit_borrowed(b.finish(), |_, _| panic!("invalid header reached callback"))
                .is_err()
        );
        assert_eq!(digest(&path), before);
        assert_eq!(j.retained_encode_bytes(), retained);
        let mut b = wal_codec_v2::RecipeBuilder::new(h(&j), &candidate, &mut workspace).unwrap();
        b.literal(0, 0).unwrap();
        j.commit_borrowed(b.finish(), |_, _| Ok(())).unwrap();
        assert_eq!(j.state().selected().lsn, 1);
        assert_eq!(j.retained_encode_bytes(), retained);
        let old = candidate.activate();
        assert_eq!(old.member_count(), 1);
        drop(j);
        let (j, ()) = Journal::open_checked(&path, |r| {
            while r.next_record()?.is_some() {}
            Ok(())
        })
        .unwrap();
        assert_eq!(j.state().selected().lsn, 1);
        assert_eq!(j.retained_encode_bytes(), 0);
        drop(j);
        std::fs::remove_file(path).unwrap();
    }
}
