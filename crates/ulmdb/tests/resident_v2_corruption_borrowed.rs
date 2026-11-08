//! Fail-closed selected data and typed-graph recovery, including correctly rehashed input.
use sha2::{Digest, Sha256};
use std::{
    fs::{self, OpenOptions},
    path::{Path, PathBuf},
};
use ulmdb::{
    DatabaseKind, Error,
    resident::{Options, RedoFormat},
};
use ulmdb_platform::FileExt;
use ulmdb_storage::resident::{
    Config,
    wal_codec_v2::{self as wal_codec, Body, Boot},
};
fn options() -> Options {
    Options {
        pool: Config {
            kv_pages: 16,
            tiny_pages: 16,
            max_reserved_bytes: 64 << 20,
            max_metadata_bytes: 64 << 20,
            max_views: 32,
        },
        wal_capacity: 4 << 20,
        graph_read_budget: 1_000_000,
        auto_collect_below: 0,
    }
}
struct Dir(PathBuf);
impl Dir {
    fn new(base: &Path) -> Self {
        Self(base.join(format!(
            "resident-corrupt-{:032x}",
            ulmdb_storage::new_identity().unwrap()
        )))
    }
}
impl Drop for Dir {
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.0);
    }
}
fn seeded(path: &Path) -> Environment {
    let e = Environment::create_with_redo(path, options(), RedoFormat::CompactV2).unwrap();
    let mut w = e.begin_write().unwrap();
    w.create_database(b"kv", DatabaseKind::Kv).unwrap();
    w.put(b"kv", b"key", b"old").unwrap();
    w.commit().unwrap();
    e
}
fn u64at(b: &[u8], at: usize) -> u64 {
    u64::from_le_bytes(b[at..at + 8].try_into().unwrap())
}
fn boot(b: &[u8]) -> Boot {
    Boot {
        uuid: b[16..32].try_into().unwrap(),
        epoch: u64at(b, 32),
        commit: u128::from_le_bytes(b[40..56].try_into().unwrap()),
        kv_pages: u32::from_le_bytes(b[56..60].try_into().unwrap()),
        tiny_pages: u32::from_le_bytes(b[60..64].try_into().unwrap()),
    }
}
fn latest(b: &[u8]) -> usize {
    if u64at(b, 65536 + 40) > u64at(b, 131072 + 40) {
        65536
    } else {
        131072
    }
}
fn sha(domain: &[u8], b: &[u8]) -> [u8; 32] {
    let mut h = Sha256::new();
    h.update(domain);
    h.update(b);
    h.finalize().into()
}
#[test]
fn acknowledged_body_corruption_never_falls_back_to_older_witness() {
    for base in [std::env::temp_dir(), PathBuf::from("/dev/shm")] {
        let d = Dir::new(&base);
        let e = seeded(&d.0);
        drop(e);
        let p = d.0.join("RESIDENT.wal");
        let mut b = fs::read(&p).unwrap();
        let h = latest(&b);
        let at = u64at(&b, h + 56) as usize;
        b[at + 300] ^= 1;
        fs::write(&p, &b).unwrap();
        assert!(matches!(
            Environment::open(&d.0, options()),
            Err(Error::Corrupt(_))
        ));
        assert_eq!(fs::read(p).unwrap(), b);
    }
}
#[test]
fn authenticated_but_malformed_selected_catalog_is_rejected_before_heal_or_clear() {
    for base in [std::env::temp_dir(), PathBuf::from("/dev/shm")] {
        for torn in [false, true] {
            let d = Dir::new(&base);
            let e = seeded(&d.0);
            drop(e);
            let p = d.0.join("RESIDENT.wal");
            let mut b = fs::read(&p).unwrap();
            let h = latest(&b);
            let start = u64at(&b, h + 56) as usize;
            let len = u64at(&b, h + 64) as usize;
            let mut record = wal_codec::decode(boot(&b), &b[start..start + len], start as u64)
                .unwrap()
                .into_parts()
                .0;
            let key = wal_codec::catalog_key(boot(&b), record.root)
                .unwrap()
                .unwrap();
            if let Body::Literal(bytes) = &mut record
                .allocations
                .iter_mut()
                .find(|a| a.destination == key)
                .unwrap()
                .body
            {
                bytes[0] ^= 0x80;
            } else {
                panic!("initial catalog must be literal");
            }
            let encoded = wal_codec::encode(boot(&b), &record).unwrap();
            assert_eq!(encoded.bytes.len(), len);
            b[start..start + len].copy_from_slice(&encoded.bytes);
            b[h + 72..h + 104].copy_from_slice(&encoded.digest);
            let digest = sha(b"ULMDB/resident/WAL-witness/v2\0", &b[h..h + 224]);
            b[h + 224..h + 256].copy_from_slice(&digest);
            if torn {
                b[h] ^= 1;
            } else {
                let last = b.len() - 1;
                b[last] = 0x77;
            }
            fs::write(&p, &b).unwrap();
            assert!(matches!(
                Environment::open(&d.0, options()),
                Err(Error::Corrupt(_))
            ));
            assert_eq!(fs::read(p).unwrap(), b);
        }
    }
}
#[test]
fn active_cached_witness_or_capacity_drift_quarantines_before_candidate_drop() {
    for base in [std::env::temp_dir(), PathBuf::from("/dev/shm")]
        .into_iter()
        .filter(|base| base.is_dir())
    {
        for capacity in [None, Some((4 << 20) - 4096), Some((4 << 20) + 4096)] {
            let d = Dir::new(&base);
            let e = seeded(&d.0);
            let old = e.begin_read();
            let held = old.get(b"kv", b"key").unwrap().unwrap();
            let p = d.0.join("RESIDENT.wal");
            let f = OpenOptions::new().read(true).write(true).open(&p).unwrap();
            if let Some(capacity) = capacity {
                f.set_len(capacity).unwrap();
            } else {
                f.write_all_at(&[0], 65536).unwrap();
            }
            f.sync_all().unwrap();
            let mut w = e.begin_write().unwrap();
            w.put(b"kv", b"key", b"new").unwrap();
            assert!(matches!(w.commit(), Err(Error::Corrupt(_))));
            assert!(e.pool_stats().unwrap().poisoned);
            assert!(matches!(e.begin_write(), Err(Error::OutcomeUnknown)));
            assert_eq!(held, b"old");
            assert_eq!(
                e.begin_read().get(b"kv", b"key").unwrap(),
                Some(b"old".as_slice())
            );
        }
    }
}
#[test]
fn rehashed_copy_with_stale_source_generation_is_rejected_before_heal_or_clear() {
    for base in [std::env::temp_dir(), PathBuf::from("/dev/shm")] {
        for torn in [false, true] {
            let d = Dir::new(&base);
            let e = seeded(&d.0);
            let mut w = e.begin_write().unwrap();
            w.put(b"kv", b"key", b"new").unwrap();
            w.commit().unwrap();
            drop(e);
            let p = d.0.join("RESIDENT.wal");
            let mut b = fs::read(&p).unwrap();
            let h = latest(&b);
            let start = u64at(&b, h + 56) as usize;
            let len = u64at(&b, h + 64) as usize;
            let mut record = wal_codec::decode(boot(&b), &b[start..start + len], start as u64)
                .unwrap()
                .into_parts()
                .0;
            let source = record
                .allocations
                .iter_mut()
                .find_map(|a| match &mut a.body {
                    Body::Copy { source, .. } => Some(source),
                    _ => None,
                })
                .expect("actual live KV transaction must create compact copy");
            source.generation += 1;
            let encoded = wal_codec::encode(boot(&b), &record).unwrap();
            assert_eq!(encoded.bytes.len(), len);
            b[start..start + len].copy_from_slice(&encoded.bytes);
            b[h + 72..h + 104].copy_from_slice(&encoded.digest);
            let digest = sha(b"ULMDB/resident/WAL-witness/v2\0", &b[h..h + 224]);
            b[h + 224..h + 256].copy_from_slice(&digest);
            if torn {
                b[h] ^= 1;
            }
            fs::write(&p, &b).unwrap();
            assert!(matches!(
                Environment::open(&d.0, options()),
                Err(Error::Corrupt(_))
            ));
            assert_eq!(fs::read(p).unwrap(), b);
        }
    }
}

#[path = "support/resident_borrowed.rs"]
mod borrowed_policy;
use borrowed_policy::Environment;
