//! Existing KV/radix operations in mapped tails; no durability qualification.
mod image_support;
use image_support::*;
use std::{
    collections::BTreeMap,
    fs::{self, File},
    sync::{Arc, Barrier},
};
use ulmdb_core::{
    Address, Prefix,
    storage::{Error, ReadStore, Reference, WriteStore},
};
use ulmdb_storage::hybrid::{ChunkWriter, ImageTable};
#[test]
fn kv_radix_same_sid_old_readers_and_fresh_incarnation() {
    filesystems(|f| {
        let path = f.file("chunk");
        let mut writer = ChunkWriter::create(&path, 17, limits(8 * 1024 * 1024)).unwrap();
        let initial = ImageTable::default()
            .with_active(writer.read_view())
            .unwrap();
        let mut a = writer.begin(initial.clone()).unwrap();
        let mut kv = Reference::NULL;
        let mut v4 = Reference::NULL;
        let mut v6 = Reference::NULL;
        let mut model = BTreeMap::new();
        for n in 0..512u32 {
            let k = n.to_be_bytes().to_vec();
            let v = (n * 17 + 1).to_be_bytes().to_vec();
            kv = ulmdb_kv::put(&mut a, kv, &k, &v).unwrap();
            model.insert(k, v);
        }
        let long = vec![b'z'; 3000];
        kv = ulmdb_kv::put(&mut a, kv, &long, &vec![3; 10000]).unwrap();
        model.insert(long.clone(), vec![3; 10000]);
        let mut four = Vec::new();
        let mut six = Vec::new();
        for len in 0..=32u8 {
            let net = if len == 0 {
                0
            } else {
                0x89ab_cdefu32 & (u32::MAX << (32 - len))
            };
            let p = Prefix::new(Address::V4(net), len).unwrap();
            let value = vec![len];
            v4 = ulmdb_prefix::insert(&mut a, v4, p, &value).unwrap();
            four.push((p, value));
        }
        for len in 0..=128u8 {
            let net = if len == 0 {
                0
            } else {
                0x20010db8000000000123456789abcdefu128 & (u128::MAX << (128 - len))
            };
            let p = Prefix::new(Address::V6(net), len).unwrap();
            let value = vec![len];
            v6 = ulmdb_prefix::insert(&mut a, v6, p, &value).unwrap();
            six.push((p, value));
        }
        let frozen = a.freeze().unwrap();
        assert!(matches!(writer.begin(initial.clone()), Err(Error::Busy)));
        let table = initial.with_active(frozen.view()).unwrap();
        let accepted = frozen.accept();
        assert!(Arc::ptr_eq(table.active().unwrap(), &accepted));
        assert!(initial.read(kv, 4096).is_err());
        kv_check(&table, kv, &model);
        let oldroots = (kv, v4, v6);
        let oldmodel = model.clone();
        let oldfour = four.clone();
        let oldsix = six.clone();
        let held = ulmdb_kv::get(&table, kv, &7u32.to_be_bytes())
            .unwrap()
            .unwrap();
        let held_bytes = held.to_vec();
        let barrier = Barrier::new(2);
        let newtable = std::thread::scope(|scope| {
            let check = scope.spawn(|| {
                barrier.wait();
                for _ in 0..2000 {
                    assert_eq!(held, held_bytes);
                    assert_eq!(
                        ulmdb_kv::get(&table, oldroots.0, &7u32.to_be_bytes()).unwrap(),
                        Some(held)
                    );
                }
                barrier.wait();
            });
            barrier.wait();
            let mut a = writer.begin(table.clone()).unwrap();
            assert_eq!(a.private_extent(kv), None);
            kv = ulmdb_kv::put(&mut a, kv, &7u32.to_be_bytes(), b"NEW!").unwrap();
            model.insert(7u32.to_be_bytes().to_vec(), b"NEW!".to_vec());
            for n in (0..512u32).step_by(3) {
                kv = ulmdb_kv::delete(&mut a, kv, &n.to_be_bytes()).unwrap();
                model.remove(n.to_be_bytes().as_slice());
            }
            for n in 512..768u32 {
                kv = ulmdb_kv::put(&mut a, kv, &n.to_be_bytes(), b"").unwrap();
                model.insert(n.to_be_bytes().to_vec(), vec![]);
            }
            let p = four[24].0;
            v4 = ulmdb_prefix::insert(&mut a, v4, p, b"changed").unwrap();
            four[24].1 = b"changed".to_vec();
            let p = six[128].0;
            v6 = ulmdb_prefix::delete(&mut a, v6, p).unwrap();
            six.pop();
            let frozen = a.freeze().unwrap();
            let next = table.with_active(frozen.view()).unwrap();
            frozen.accept();
            barrier.wait();
            check.join().unwrap();
            next
        });
        assert_eq!(held, held_bytes);
        kv_check(&table, oldroots.0, &oldmodel);
        kv_check(&newtable, kv, &model);
        let q4: Vec<_> = (0..32)
            .map(|i| Address::V4(0x89ab_cdefu32 ^ (1 << i)))
            .chain([Address::V4(0x89ab_cdef)])
            .collect();
        let q6: Vec<_> = (0..128)
            .map(|i| Address::V6(0x20010db8000000000123456789abcdefu128 ^ (1 << i)))
            .chain([Address::V6(0x20010db8000000000123456789abcdef)])
            .collect();
        radix_check(&table, oldroots.1, &oldfour, &q4);
        radix_check(&table, oldroots.2, &oldsix, &q6);
        radix_check(&newtable, v4, &four, &q4);
        radix_check(&newtable, v6, &six, &q6);
        let source = Arc::clone(newtable.active().unwrap());
        let old_incarnation = source.image_incarnation();
        drop(writer);
        let lock = File::open(&path).unwrap();
        lock.try_lock().unwrap();
        drop(lock);
        let mut reopened =
            ChunkWriter::create_from(f.file("fresh"), limits(8 * 1024 * 1024), &source).unwrap();
        assert_ne!(reopened.image_incarnation(), old_incarnation);
        let recovered = ImageTable::default()
            .with_active(reopened.read_view())
            .unwrap();
        kv_check(&recovered, kv, &model);
        radix_check(&recovered, v6, &six, &q6);
        let mut a = reopened.begin(recovered.clone()).unwrap();
        let updated = ulmdb_kv::put(&mut a, kv, &7u32.to_be_bytes(), b"again").unwrap();
        let frozen = a.freeze().unwrap();
        let final_table = recovered.with_active(frozen.view()).unwrap();
        frozen.accept();
        assert_eq!(
            ulmdb_kv::get(&final_table, updated, &7u32.to_be_bytes()).unwrap(),
            Some(b"again".as_slice())
        );
        assert_eq!(
            ulmdb_kv::get(&newtable, kv, &7u32.to_be_bytes()).unwrap(),
            Some(b"NEW!".as_slice())
        );
        assert_eq!(held, held_bytes);
        assert_eq!(fs::metadata(path).unwrap().len(), 8 * 1024 * 1024);
    });
}

#[test]
fn savepoints_restore_exact_private_bytes_and_same_sid_copies() {
    filesystems(|f| {
        let mut w = ChunkWriter::create(f.file("chunk"), 1, limits(1024 * 1024)).unwrap();
        let base = ImageTable::default().with_active(w.read_view()).unwrap();
        let mut a = w.begin(base.clone()).unwrap();
        let mut root = ulmdb_kv::put(&mut a, Reference::NULL, b"key", b"base").unwrap();
        let value = a.put_payload(b"private").unwrap();
        let revision = a.revision();
        let outer = a.checkpoint().unwrap();
        root = ulmdb_kv::put(&mut a, root, b"key", b"edit").unwrap();
        a.replace_payload(value, b"changed").unwrap();
        let inner = a.checkpoint().unwrap();
        let extra = a.allocate(b"tail", 16).unwrap();
        a.patch(value, 23, 16, b"CHANGED", 16).unwrap();
        assert_eq!(a.payload(value).unwrap(), b"CHANGED");
        a.release_checkpoint(&inner).unwrap();
        a.rollback_checkpoint(&outer).unwrap();
        assert_eq!(a.revision(), revision);
        assert_eq!(a.payload(value).unwrap(), b"private");
        assert!(a.read(extra, 4).is_err());
        assert_eq!(
            ulmdb_kv::get(&a, root, b"key").unwrap(),
            Some(b"base".as_slice())
        );
        let frozen = a.freeze().unwrap();
        let table = base.with_active(frozen.view()).unwrap();
        frozen.accept();
        let held = table.payload(value).unwrap();
        let mut a = w.begin(table.clone()).unwrap();
        assert!(a.private_extent(value).is_none());
        let next = a.patch(value, 23, 16, b"another", 16).unwrap();
        assert_eq!(next.segment, value.segment);
        assert_ne!(next.offset, value.offset);
        assert_eq!(a.payload(next).unwrap(), b"another");
        let allocations = a.freeze().unwrap();
        assert_eq!(
            allocations.allocations().next().unwrap().base_source,
            Some(value)
        );
        let escaped = allocations.view();
        let end = allocations.end();
        drop(allocations);
        assert_eq!(
            escaped.read(next.offset as usize, 23).unwrap()[16..],
            *b"another"
        );
        let mut a = w.begin(table.clone()).unwrap();
        let later = a.put_payload(b"later").unwrap();
        assert!(later.offset >= end as u64);
        drop(a);
        assert_eq!(held, b"private");
        assert_eq!(
            escaped.read(next.offset as usize + 16, 7).unwrap(),
            b"another"
        );
        assert!(w.status().burned_bytes >= 27);
        assert_eq!(w.status().published_end, table.active().unwrap().end());
    });
}

#[test]
fn owned_mutable_and_frozen_leases_retain_flock_but_read_views_do_not() {
    filesystems(|f| {
        let path = f.file("chunk");
        let mut w = ChunkWriter::create(&path, 2, limits(1024 * 1024)).unwrap();
        let base = ImageTable::default().with_active(w.read_view()).unwrap();
        let mut a = w.begin(base.clone()).unwrap();
        let at = a.allocate(b"readers", 16).unwrap();
        assert!(matches!(w.begin(base.clone()), Err(Error::Busy)));
        drop(w);
        let other = File::open(&path).unwrap();
        assert!(matches!(
            other.try_lock(),
            Err(std::fs::TryLockError::WouldBlock)
        ));
        let frozen = a.freeze().unwrap();
        assert!(matches!(
            other.try_lock(),
            Err(std::fs::TryLockError::WouldBlock)
        ));
        let escaped = frozen.view();
        drop(frozen);
        other.try_lock().unwrap();
        assert_eq!(escaped.read(at.offset as usize, 7).unwrap(), b"readers");
    });
}

#[test]
fn bounded_abort_debt_capacity_and_unpublished_state() {
    filesystems(|f| {
        let mut policy = limits(65536);
        policy.max_burned_bytes = 16;
        let mut w = ChunkWriter::create(f.file("chunk"), 3, policy).unwrap();
        let base = ImageTable::default().with_active(w.read_view()).unwrap();
        let old_end = w.status().published_end;
        let mut a = w.begin(base.clone()).unwrap();
        let at = a.allocate(&[5; 32], 16).unwrap();
        let frozen = a.freeze().unwrap();
        let escaped = frozen.view();
        drop(frozen);
        assert!(w.status().blocked);
        assert_eq!(w.status().published_end, old_end);
        assert_eq!(w.status().burned_bytes, 32);
        assert!(matches!(w.begin(base.clone()), Err(Error::LimitExceeded)));
        assert!(base.read(at, 1).is_err());
        assert_eq!(escaped.read(at.offset as usize, 32).unwrap(), [5; 32]);
        let mut w = ChunkWriter::create(f.file("capacity"), 4, limits(65536)).unwrap();
        let base = ImageTable::default().with_active(w.read_view()).unwrap();
        let mut a = w.begin(base.clone()).unwrap();
        let prior = a.allocate(b"unchanged", 16).unwrap();
        let revision = a.revision();
        assert!(matches!(
            a.allocate(&vec![0; 65536], 16),
            Err(Error::LimitExceeded)
        ));
        assert_eq!(a.revision(), revision);
        assert_eq!(a.read(prior, 9).unwrap(), b"unchanged");
        assert!(a.patch(prior, 10, 0, b"x", 16).is_err());
        assert!(a.patch(prior, 9, usize::MAX, b"x", 16).is_err());
        assert_eq!(a.read(prior, 9).unwrap(), b"unchanged");
        let token = a.checkpoint().unwrap();
        assert!(a.freeze().is_err());
        assert!(w.begin(base.clone()).is_ok());
        drop(token);
    });
}

#[test]
fn real_kv_supports_configured_large_value_class_and_zero_length() {
    filesystems(|f| {
        let mut w = ChunkWriter::create(f.file("large"), 5, limits(128 * 1024 * 1024)).unwrap();
        let base = ImageTable::default().with_active(w.read_view()).unwrap();
        let bytes = vec![0x67; 64 * 1024 * 1024];
        let mut a = w.begin(base.clone()).unwrap();
        let empty = a.allocate(&[], 16).unwrap();
        assert_eq!(a.private_extent(empty), Some(0));
        let root = ulmdb_kv::put(&mut a, Reference::NULL, b"large", &bytes).unwrap();
        let frozen = a.freeze().unwrap();
        let table = base.with_active(frozen.view()).unwrap();
        frozen.accept();
        let held = ulmdb_kv::get(&table, root, b"large").unwrap().unwrap();
        assert!(held == bytes);
        let mut a = w.begin(table.clone()).unwrap();
        let next = ulmdb_kv::put(&mut a, root, b"large", b"").unwrap();
        let frozen = a.freeze().unwrap();
        let changed = table.with_active(frozen.view()).unwrap();
        frozen.accept();
        assert_eq!(
            ulmdb_kv::get(&changed, next, b"large").unwrap(),
            Some(b"".as_slice())
        );
        assert!(held == bytes);
    });
}

#[test]
fn undo_budget_and_foreign_tokens_fail_before_private_changes() {
    filesystems(|f| {
        let mut w = ChunkWriter::create(f.file("budget"), 6, limits(64 * 1024 * 1024)).unwrap();
        let base = ImageTable::default().with_active(w.read_view()).unwrap();
        let mut a = w.begin(base).unwrap();
        let value = a.put_payload(&vec![9; 48 * 1024 * 1024]).unwrap();
        let mut tokens = Vec::new();
        for _ in 0..6 {
            tokens.push(a.checkpoint().unwrap());
        }
        let revision = a.revision();
        assert!(matches!(
            a.patch(value, 48 * 1024 * 1024 + 16, 16, b"bad", 16),
            Err(Error::LimitExceeded)
        ));
        assert_eq!(a.savepoint_undo_bytes(), 0);
        assert_eq!(a.savepoint_depth(), 6);
        assert_eq!(a.revision(), revision);
        assert_eq!(&a.payload(value).unwrap()[..3], &[9; 3]);
        let mut other = ChunkWriter::create(f.file("other"), 7, limits(65536)).unwrap();
        let mut b = other.begin(ImageTable::default()).unwrap();
        let foreign = b.checkpoint().unwrap();
        assert!(a.rollback_checkpoint(&foreign).is_err());
        b.rollback_checkpoint(&foreign).unwrap();
        for token in tokens.iter().rev() {
            a.release_checkpoint(token).unwrap();
        }
        a.freeze().unwrap().accept();
    });
}

#[test]
fn global_environment_fence_is_retained_only_by_mutation_authority() {
    filesystems(|f| {
        let path = f.file("LOCK");
        let global = Arc::new(
            ulmdb_storage::WriterFence::try_lock(
                fs::OpenOptions::new()
                    .create_new(true)
                    .read(true)
                    .write(true)
                    .open(&path)
                    .unwrap(),
            )
            .unwrap(),
        );
        let mut w =
            ChunkWriter::create_with_fence(f.file("chunk"), 8, limits(65536), Arc::clone(&global))
                .unwrap();
        let base = ImageTable::default().with_active(w.read_view()).unwrap();
        let mut a = w.begin(base).unwrap();
        let at = a.allocate(b"value", 16).unwrap();
        drop(w);
        drop(global);
        let contender = File::open(path).unwrap();
        assert!(matches!(
            contender.try_lock(),
            Err(std::fs::TryLockError::WouldBlock)
        ));
        let frozen = a.freeze().unwrap();
        let view = frozen.view();
        assert!(matches!(
            contender.try_lock(),
            Err(std::fs::TryLockError::WouldBlock)
        ));
        drop(frozen);
        contender.try_lock().unwrap();
        assert_eq!(view.read(at.offset as usize, 5).unwrap(), b"value");
    });
}

#[test]
fn malformed_geometry_short_base_and_private_nodes_fail_closed() {
    filesystems(|f| {
        for (name, id, policy) in [
            ("zero", 0, limits(65536)),
            ("max", u64::MAX, limits(65536)),
            ("capacity", 1, limits(0)),
        ] {
            let p = f.file(name);
            assert!(ChunkWriter::create(&p, id, policy).is_err());
            assert!(!p.exists());
        }
        let mut w = ChunkWriter::create(f.file("chunk"), 10, limits(65536)).unwrap();
        let base = ImageTable::default().with_active(w.read_view()).unwrap();
        let mut a = w.begin(base).unwrap();
        let bad = a.allocate(&[0; 4096], 4096).unwrap();
        let bytes = a.private_bytes();
        let revision = a.revision();
        assert!(matches!(
            ulmdb_kv::put(&mut a, bad, b"key", b"value"),
            Err(Error::Corrupt(_))
        ));
        assert_eq!(a.private_bytes(), bytes);
        assert_eq!(a.revision(), revision);
        let bad_prefix = a.allocate(&[0; 96], 16).unwrap();
        let bytes = a.private_bytes();
        assert!(matches!(
            ulmdb_prefix::insert(
                &mut a,
                bad_prefix,
                Prefix::new(Address::V4(0), 0).unwrap(),
                b"x"
            ),
            Err(Error::Corrupt(_))
        ));
        assert_eq!(a.private_bytes(), bytes);
        assert!(matches!(
            a.read(
                Reference {
                    segment: 10,
                    offset: u64::MAX
                },
                2
            ),
            Err(Error::Corrupt(_))
        ));
        drop(a);
        struct Short;
        impl ReadStore for Short {
            fn read(&self, _: Reference, _: usize) -> Result<&[u8], Error> {
                Ok(b"short")
            }
        }
        let mut a = w.begin(Short).unwrap();
        let old = Reference {
            segment: 9,
            offset: 4096,
        };
        assert!(matches!(
            a.patch(old, 4096, 0, b"x", 4096),
            Err(Error::Corrupt(_))
        ));
        assert!(matches!(
            a.rewrite(old, b"longer", 1),
            Err(Error::Corrupt(_))
        ));
        assert_eq!(a.private_bytes(), 0);
    });
}

#[test]
fn active_view_cannot_shrink_or_switch_physical_owner() {
    filesystems(|f| {
        let mut w = ChunkWriter::create(f.file("chunk"), 11, limits(65536)).unwrap();
        let initial = w.read_view();
        let table = ImageTable::default()
            .with_active(Arc::clone(&initial))
            .unwrap();
        let mut a = w.begin(table.clone()).unwrap();
        let value = a.put_payload(b"stable").unwrap();
        let frozen = a.freeze().unwrap();
        let current = table.with_active(frozen.view()).unwrap();
        frozen.accept();
        assert!(matches!(
            current.with_active(initial),
            Err(Error::InvalidInput(_))
        ));
        let other =
            ChunkWriter::create_from(f.file("other"), limits(65536), current.active().unwrap())
                .unwrap();
        assert!(matches!(
            current.with_active(other.read_view()),
            Err(Error::InvalidInput(_))
        ));
        assert_eq!(current.payload(value).unwrap(), b"stable");
    });
}

#[test]
fn physical_afterimages_replay_at_exact_offsets_without_rerunning_indexes() {
    filesystems(|f| {
        let mut writer = ChunkWriter::create(f.file("original"), 12, limits(1024 * 1024)).unwrap();
        let initial = ImageTable::default()
            .with_active(writer.read_view())
            .unwrap();
        let mut a = writer.begin(initial.clone()).unwrap();
        let mut root = Reference::NULL;
        let mut prefix = Reference::NULL;
        let mut model = BTreeMap::new();
        for n in 0..64u32 {
            let k = n.to_be_bytes().to_vec();
            root = ulmdb_kv::put(&mut a, root, &k, b"checkpoint").unwrap();
            model.insert(k, b"checkpoint".to_vec());
        }
        let p = Prefix::new(Address::V6(0x20010db8000000000000000000000000), 64).unwrap();
        prefix = ulmdb_prefix::insert(&mut a, prefix, p, b"old").unwrap();
        let frozen = a.freeze().unwrap();
        let checkpoint = initial.with_active(frozen.view()).unwrap();
        frozen.accept();
        let old_root = root;
        let held = ulmdb_kv::get(&checkpoint, root, &0u32.to_be_bytes())
            .unwrap()
            .unwrap();
        let mut a = writer.begin(checkpoint.clone()).unwrap();
        root = ulmdb_kv::put(&mut a, root, &0u32.to_be_bytes(), b"changed").unwrap();
        model.insert(0u32.to_be_bytes().to_vec(), b"changed".to_vec());
        prefix = ulmdb_prefix::insert(&mut a, prefix, p, b"new").unwrap();
        let candidate = a.freeze().unwrap();
        let view = candidate.view();
        let mut fresh = ChunkWriter::create_from(
            f.file("reconstructed"),
            limits(1024 * 1024),
            checkpoint.active().unwrap(),
        )
        .unwrap();
        let base = ImageTable::default()
            .with_active(fresh.read_view())
            .unwrap();
        let mut replay = fresh.begin(base.clone()).unwrap();
        let first = candidate.allocations().next().unwrap();
        assert!(
            replay
                .allocate_at(
                    Reference {
                        segment: 13,
                        offset: first.reference.offset
                    },
                    b"bad"
                )
                .is_err()
        );
        for object in candidate.allocations() {
            let bytes = view
                .read(object.reference.offset as usize, object.length)
                .unwrap();
            assert_eq!(
                replay.allocate_at(object.reference, bytes).unwrap(),
                object.reference
            );
        }
        assert!(replay.allocate_at(first.reference, b"overlap").is_err());
        assert!(replay.advance_to(4096).is_err());
        replay.advance_to(candidate.end()).unwrap();
        let frozen = replay.freeze().unwrap();
        let restored = base.with_active(frozen.view()).unwrap();
        frozen.accept();
        kv_check(&restored, root, &model);
        assert_eq!(
            ulmdb_prefix::exact(&restored, prefix, p)
                .unwrap()
                .map(|(_, v)| v),
            Some(b"new".as_slice())
        );
        assert_eq!(held, b"checkpoint");
        assert_eq!(
            ulmdb_kv::get(&checkpoint, old_root, &0u32.to_be_bytes()).unwrap(),
            Some(held)
        );
        drop(candidate);
        assert_eq!(writer.read_view().end(), checkpoint.active().unwrap().end());
    });
}

#[test]
fn real_overflow_value_replacement_preserves_committed_copy_provenance() {
    filesystems(|f| {
        let mut writer = ChunkWriter::create(f.file("chunk"), 14, limits(1024 * 1024)).unwrap();
        let initial = ImageTable::default()
            .with_active(writer.read_view())
            .unwrap();
        let old = vec![1u8; 8192];
        let mut a = writer.begin(initial.clone()).unwrap();
        let root = ulmdb_kv::put(&mut a, Reference::NULL, b"overflow", &old).unwrap();
        let frozen = a.freeze().unwrap();
        let payload = frozen
            .allocations()
            .find(|o| o.length == 8192 + 16)
            .unwrap()
            .reference;
        let checkpoint = initial.with_active(frozen.view()).unwrap();
        frozen.accept();
        let held = ulmdb_kv::get(&checkpoint, root, b"overflow")
            .unwrap()
            .unwrap();
        let mut changed = old.clone();
        changed[4000] = 2;
        let mut a = writer.begin(checkpoint.clone()).unwrap();
        let next = ulmdb_kv::put(&mut a, root, b"overflow", &changed).unwrap();
        let frozen = a.freeze().unwrap();
        assert!(
            frozen
                .allocations()
                .any(|o| o.length == 8192 + 16 && o.base_source == Some(payload))
        );
        let current = checkpoint.with_active(frozen.view()).unwrap();
        frozen.accept();
        assert_eq!(
            ulmdb_kv::get(&current, next, b"overflow").unwrap(),
            Some(changed.as_slice())
        );
        assert_eq!(held, old);
    });
}
