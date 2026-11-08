//! H5 primitive tests: no assertion that an artifact authorizes WAL trimming.
use sha2::{Digest, Sha256};
use std::{
    fs,
    path::{Path, PathBuf},
    sync::Arc,
};
use ulmdb_storage::hybrid::{
    ChunkLimits, ChunkWriter, ImageTable,
    checkpoint::{self, CheckpointCut, CheckpointLimits, SnapshotSelection},
    journal::{Bootstrap, Journal},
    provenance::{self, ActiveProvenance, ChunkIdentity, Origin, ProvenanceDirectory},
    redo::*,
};
use ulmdb_storage::{Error, ReadStore, Reference, Result};
struct Dir(PathBuf);
impl Dir {
    fn new(base: &Path) -> Self {
        #[cfg(windows)]
        let local = [Path::new("/tmp"), Path::new("/dev/shm")]
            .contains(&base).then(std::env::temp_dir);
        #[cfg(windows)]
        let base = local.as_deref().unwrap_or(base);
        let p = base.join(format!(
            "ulmdb-h5-{}",
            ulmdb_storage::new_identity().unwrap()
        ));
        fs::create_dir(&p).unwrap();
        Self(p)
    }
}
impl Drop for Dir {
    fn drop(&mut self) {
        fs::remove_dir_all(&self.0).unwrap();
    }
}
const BOOT: Bootstrap = Bootstrap {
    uuid: [5; 16],
    epoch: 3,
    commit: 9,
};
fn identity(chunk: u64) -> ChunkIdentity {
    ChunkIdentity {
        uuid: BOOT.uuid,
        epoch: BOOT.epoch,
        chunk,
        logical_generation: 100 + u128::from(chunk),
        capacity: 1024 * 1024,
    }
}
fn origin(chunk: u64, offset: u64, lsn: u64) -> Origin {
    Origin {
        extent: Extent {
            at: Reference {
                segment: chunk,
                offset,
            },
            logical_generation: identity(chunk).logical_generation,
            length: 96,
        },
        receipt: SourceReceipt {
            uuid: BOOT.uuid,
            epoch: BOOT.epoch,
            creation_lsn: lsn,
            creation_sequence: lsn,
            creation_commit: u128::from(lsn) + 1000,
            provenance_digest: [lsn as u8; 32],
            authority: Authority::Wal {
                lsn,
                sequence: lsn,
                digest: [lsn as u8; 32],
            },
        },
    }
}
fn table(chunk: u64, count: u64) -> Arc<provenance::ChunkProvenance> {
    let mut a = ActiveProvenance::new(identity(chunk)).unwrap();
    for n in 0..count {
        a.insert(origin(chunk, 4096 + n * 96, 1)).unwrap();
    }
    a.seal(4096 + count * 96, 1, 1).unwrap()
}
#[test]
fn compact_origins_are_lossless_and_deduplicate_creation_metadata() {
    let mut a = ActiveProvenance::new(identity(1)).unwrap();
    for n in 0..1000 {
        let mut o = origin(1, 4096 + n * 96, 1);
        if n % 2 == 1 {
            o.receipt.authority = Authority::Checkpoint {
                generation: 77,
                sequence: 2,
                digest: [19; 32],
            };
        }
        a.insert(o).unwrap();
    }
    let t = a.seal(4096 + 1000 * 96, 2, 2).unwrap();
    let mut bytes = Vec::new();
    let stats = provenance::encode(&t, &mut bytes).unwrap();
    assert_eq!(stats.origins, 1);
    assert_eq!(stats.checkpoint_authorities, 1);
    assert_eq!(bytes.len(), 256 + 64 + 64 + 1000 * 16 + 32);
    let decoded = provenance::decode(&bytes).unwrap();
    assert_eq!(decoded.identity(), t.identity());
    assert_eq!(decoded.bounds(), t.bounds());
    assert!(decoded.origins().eq(t.origins()));
}
fn rehash(bytes: &mut [u8]) {
    let n = bytes.len();
    let mut h = Sha256::new();
    h.update(b"ULMDB/hybrid/chunk-provenance/v1\0");
    h.update(&bytes[..n - 32]);
    bytes[n - 32..].copy_from_slice(&h.finalize());
}
#[test]
fn malformed_compact_origins_fail_closed_even_with_correct_hash() {
    let t = table(1, 2);
    let mut original = Vec::new();
    provenance::encode(&t, &mut original).unwrap();
    // Header reserved, count overflow, dictionary index, authority index,
    // overlapping extent, zero width, false aggregate source floor.
    for (offset, value) in [
        (144, 1),
        (80, 255),
        (256 + 64 + 8, 5),
        (256 + 64 + 12, 1),
        (256 + 64 + 16, 0),
        (256 + 64 + 4, 0),
        (136, 0),
    ] {
        let mut bad = original.clone();
        bad[offset] = value;
        rehash(&mut bad);
        assert!(provenance::decode(&bad).is_err(), "offset={offset}");
    }
    for bit in 0..8 {
        let mut bad = original.clone();
        bad[8] ^= 1 << bit;
        rehash(&mut bad);
        assert!(provenance::decode(&bad).is_err());
    }
    let mut bad = original.clone();
    bad.push(0);
    assert!(provenance::decode(&bad).is_err());
    assert!(provenance::decode(&original[..original.len() - 1]).is_err());
}
#[test]
fn sealed_directory_retains_old_roots_and_handles_full_u64_key_space() {
    let mut directory = ProvenanceDirectory::default();
    let mut saved = Vec::new();
    for id in [1, 15, 16, 255, 256, 1 << 40, (1 << 63) + 1, u64::MAX - 1] {
        let t = table(id, 1);
        saved.push(directory.clone());
        directory = directory.insert(Arc::clone(&t)).unwrap();
        assert!(Arc::ptr_eq(directory.get(id).unwrap(), &t));
        assert!(saved.last().unwrap().get(id).is_none());
    }
    assert_eq!(
        directory
            .iter()
            .map(|t| t.identity().chunk)
            .collect::<Vec<_>>(),
        [1, 15, 16, 255, 256, 1 << 40, (1 << 63) + 1, u64::MAX - 1]
    );
    assert!(directory.insert(table(1, 1)).is_err());
    assert_eq!(saved[0].len(), 0);
}
#[test]
fn seal_rejects_pending_future_objects_and_bad_authority() {
    let mut a = ActiveProvenance::new(identity(1)).unwrap();
    a.insert(origin(1, 4096, 2)).unwrap();
    assert!(a.seal(4192, 1, 1).is_err());
    let mut a = ActiveProvenance::new(identity(1)).unwrap();
    let mut o = origin(1, 4096, 1);
    o.receipt.authority = Authority::Wal {
        lsn: 2,
        sequence: 1,
        digest: [1; 32],
    };
    assert!(a.insert(o).is_err());
    a.insert(origin(1, 4096, 1)).unwrap();
    assert!(a.insert(origin(1, 4097, 1)).is_err());
    assert!(a.seal(4096, 1, 1).is_err());
}
struct NoSources;
impl SourceResolver for NoSources {
    fn resolve_committed<'a>(&'a self, _: &Extent, _: &SourceReceipt) -> Result<&'a [u8]> {
        Err(Error::Corrupt("unexpected base copy"))
    }
}
fn selected(j: &Journal) -> SnapshotSelection {
    SnapshotSelection {
        receipt: j.selected(),
        commit: j.commit_id(),
        root: j.root(),
    }
}
fn append(
    j: &mut Journal,
    w: &mut ChunkWriter,
    base: &ImageTable,
    root: Reference,
    n: u64,
) -> (ImageTable, Arc<provenance::ChunkProvenance>, Reference) {
    let mut arena = w.begin(base.clone()).unwrap();
    let mut root = root;
    for k in 0..32 {
        root = ulmdb_kv::put(
            &mut arena,
            root,
            &(n * 100 + k).to_be_bytes(),
            &[k as u8; 400],
        )
        .unwrap();
    }
    let frozen = arena.freeze().unwrap();
    let image = base.with_active(frozen.view()).unwrap();
    let id = identity(w.chunk_id());
    let objects = frozen
        .allocations()
        .map(|a| ObjectRedo::Literal {
            dest: Extent {
                at: a.reference,
                logical_generation: id.logical_generation,
                length: a.length as u32,
            },
            bytes: image.read(a.reference, a.length).unwrap().to_vec(),
        })
        .collect();
    let record = Record {
        uuid: BOOT.uuid,
        epoch: BOOT.epoch,
        lsn: j.selected().lsn + 1,
        sequence: j.selected().sequence + 1,
        commit: 10000 + u128::from(n),
        parent_commit: j.commit_id(),
        parent: j.selected(),
        root: RootState {
            catalog: root,
            next_chunk: w.chunk_id() + 1,
            next_database: 2,
        },
        chunks: vec![ChunkDelta {
            chunk: w.chunk_id(),
            logical_generation: id.logical_generation,
            before: frozen.start() as u64,
            after: frozen.end() as u64,
            capacity: id.capacity,
        }],
        objects,
    };
    let receipt = j.commit_checked(&record, &NoSources, |_| Ok(())).unwrap();
    let mut active = ActiveProvenance::new(id).unwrap();
    for a in frozen.allocations() {
        active
            .insert(Origin {
                extent: Extent {
                    at: a.reference,
                    logical_generation: id.logical_generation,
                    length: a.length as u32,
                },
                receipt: SourceReceipt {
                    uuid: BOOT.uuid,
                    epoch: BOOT.epoch,
                    creation_lsn: receipt.lsn,
                    creation_sequence: receipt.sequence,
                    creation_commit: record.commit,
                    provenance_digest: receipt.digest,
                    authority: Authority::Wal {
                        lsn: receipt.lsn,
                        sequence: receipt.sequence,
                        digest: receipt.digest,
                    },
                },
            })
            .unwrap();
    }
    let metadata = active
        .seal(frozen.end() as u64, receipt.lsn, receipt.sequence)
        .unwrap();
    frozen.accept();
    (image, metadata, root)
}
#[test]
fn selected_old_cuts_build_durable_copies_on_disk_and_tmpfs_without_touching_wal() {
    for base in [Path::new("/tmp"), Path::new("/dev/shm")] {
        let d = Dir::new(base);
        let wal = d.0.join("WAL");
        let mut j = Journal::create(&wal, BOOT).unwrap();
        let limits = ChunkLimits {
            capacity: 1024 * 1024,
            reserve_quantum: 65536,
            max_burned_bytes: 65536,
        };
        let mut one = ChunkWriter::create(d.0.join("one"), 1, limits).unwrap();
        let (first, m1, r1) = append(&mut j, &mut one, &ImageTable::default(), Reference::NULL, 1);
        let directory = ProvenanceDirectory::default().insert(m1).unwrap();
        let cut = CheckpointCut::capture(&j, selected(&j), &first, &directory).unwrap();
        let mut wrong = selected(&j);
        wrong.commit += 1;
        assert!(CheckpointCut::capture(&j, wrong, &first, &directory).is_err());
        let mut two = ChunkWriter::create(d.0.join("two"), 2, limits).unwrap();
        let (second, m2, r2) = append(&mut j, &mut two, &first, r1, 2);
        let directory2 = directory.insert(m2).unwrap();
        assert_eq!(
            ulmdb_kv::get(&first, r1, &100u64.to_be_bytes()).unwrap(),
            Some([0u8; 400].as_slice())
        );
        assert!(
            ulmdb_kv::get(&first, r1, &200u64.to_be_bytes())
                .unwrap()
                .is_none()
        );
        assert!(
            ulmdb_kv::get(&second, r2, &200u64.to_be_bytes())
                .unwrap()
                .is_some()
        );
        let cut2 = CheckpointCut::capture(&j, selected(&j), &second, &directory2).unwrap();
        let before = fs::read(&wal).unwrap();
        drop(one);
        drop(two);
        // Mapped owners remain valid after cache paths disappear; build consumes
        // only generic ReadSpan bytes, compatible with anonymous image backing.
        fs::remove_file(d.0.join("one")).unwrap();
        fs::remove_file(d.0.join("two")).unwrap();
        for (name, c, count) in [("old", &cut, 1), ("new", &cut2, 2)] {
            let artifact =
                checkpoint::build(c, &d.0.join(name), 77, CheckpointLimits::default()).unwrap();
            let decoded =
                checkpoint::verify_artifact(artifact.directory(), CheckpointLimits::default())
                    .unwrap();
            assert_eq!(decoded.selected, c.selection());
            assert_eq!(decoded.chunks.len(), count);
            assert_eq!(decoded.required_wal_floor, 1);
            assert_eq!(decoded.digest, artifact.digest());
            assert!(artifact.space().allocated_bytes > 0);
            if count == 2 {
                for file in [
                    "CHECKPOINT",
                    "chunk-0000000000000001.origins",
                    "chunk-0000000000000002.image",
                ] {
                    let path = artifact.directory().join(file);
                    let original = fs::read(&path).unwrap();
                    let mut bad = original.clone();
                    let n = bad.len();
                    bad[n - 1] ^= 1;
                    let owner = fs::File::open(&path).unwrap();
                    ulmdb_platform::set_private_permissions(&owner, true).unwrap();
                    fs::write(&path, &bad).unwrap();
                    assert!(
                        checkpoint::verify_artifact(
                            artifact.directory(),
                            CheckpointLimits::default()
                        )
                        .is_err()
                    );
                    fs::write(&path, &original).unwrap();
                    ulmdb_platform::set_private_permissions(&owner, false).unwrap();
                }
                checkpoint::verify_artifact(artifact.directory(), CheckpointLimits::default())
                    .unwrap();
            }
            assert!(
                checkpoint::build(c, artifact.directory(), 78, CheckpointLimits::default())
                    .is_err()
            );
        }
        for (name, limits) in [
            (
                "too-little-space",
                CheckpointLimits {
                    max_allocated_bytes: 8192,
                    ..CheckpointLimits::default()
                },
            ),
            (
                "too-little-metadata",
                CheckpointLimits {
                    max_provenance_bytes: 1,
                    ..CheckpointLimits::default()
                },
            ),
        ] {
            assert!(checkpoint::build(&cut2, &d.0.join(name), 88, limits).is_err());
            assert!(!d.0.join(name).exists());
        }
        assert_eq!(before, fs::read(&wal).unwrap());
        drop(j);
        let (j, _) = Journal::open_checked(&wal, |p| {
            assert_eq!(p.records.len(), 2);
            Ok(())
        })
        .unwrap();
        assert_eq!(j.root().catalog, r2);
    }
}
#[test]
fn failed_private_artifact_is_removed_and_genesis_floor_does_not_claim_trim() {
    let d = Dir::new(Path::new("/tmp"));
    let wal = d.0.join("WAL");
    let j = Journal::create(&wal, BOOT).unwrap();
    let cut = CheckpointCut::capture(
        &j,
        selected(&j),
        &ImageTable::default(),
        &ProvenanceDirectory::default(),
    )
    .unwrap();
    assert_eq!(cut.required_wal_floor(), 0);
    let before = fs::read(&wal).unwrap();
    let target = d.0.join("small");
    assert!(
        checkpoint::build(
            &cut,
            &target,
            1,
            CheckpointLimits {
                max_apparent_bytes: 1,
                ..CheckpointLimits::default()
            }
        )
        .is_err()
    );
    assert!(!target.exists());
    assert_eq!(before, fs::read(&wal).unwrap());
    let target = d.0.join("empty");
    checkpoint::build(&cut, &target, 2, CheckpointLimits::default()).unwrap();
    assert!(
        checkpoint::verify_artifact(&target, CheckpointLimits::default())
            .unwrap()
            .chunks
            .is_empty()
    );
    fs::write(target.join("unexpected"), b"x").unwrap();
    assert!(checkpoint::verify_artifact(&target, CheckpointLimits::default()).is_err());
}

#[cfg(feature = "fault-injection")]
#[test]
fn h5_fault_worker() {
    let Some(root) = std::env::var_os("ULMDB_H5_WORKER_ROOT") else {
        return;
    };
    let root = PathBuf::from(root);
    let mut j = Journal::create(&root.join("WAL"), BOOT).unwrap();
    let mut w = ChunkWriter::create(
        root.join("image"),
        1,
        ChunkLimits {
            capacity: 1024 * 1024,
            reserve_quantum: 65536,
            max_burned_bytes: 65536,
        },
    )
    .unwrap();
    let (image, metadata, _) = append(&mut j, &mut w, &ImageTable::default(), Reference::NULL, 1);
    let directory = ProvenanceDirectory::default().insert(metadata).unwrap();
    let cut = CheckpointCut::capture(&j, selected(&j), &image, &directory).unwrap();
    fs::write(root.join("wal.before"), fs::read(root.join("WAL")).unwrap()).unwrap();
    let result = checkpoint::build(
        &cut,
        &root.join("artifact"),
        17,
        CheckpointLimits::default(),
    );
    if std::env::var_os("ULMDB_H5_FAIL_AT").is_some() {
        assert!(result.is_err());
        assert!(!root.join("artifact").exists());
    } else {
        result.unwrap();
    }
}
#[cfg(feature = "fault-injection")]
#[test]
fn every_artifact_io_cut_preserves_selected_wal_on_disk_and_tmpfs() {
    use std::process::Command;
    fn child(
        root: &Path,
        mode: Option<(&str, &str)>,
        trace: Option<&Path>,
    ) -> std::process::Output {
        let mut c = Command::new(std::env::current_exe().unwrap());
        c.arg("--exact")
            .arg("h5_fault_worker")
            .arg("--nocapture")
            .env("ULMDB_H5_WORKER_ROOT", root)
            .env_remove("ULMDB_H5_FAIL_AT")
            .env_remove("ULMDB_H5_EXIT_AT")
            .env_remove("ULMDB_H5_TRACE");
        if let Some((variable, point)) = mode {
            c.env(variable, point);
        }
        if let Some(path) = trace {
            c.env("ULMDB_H5_TRACE", path);
        }
        c.output().unwrap()
    }
    for base in [Path::new("/tmp"), Path::new("/dev/shm")] {
        let traced = Dir::new(base);
        let output = child(&traced.0, None, Some(&traced.0.join("trace")));
        assert!(
            output.status.success(),
            "{}",
            String::from_utf8_lossy(&output.stderr)
        );
        let points = fs::read_to_string(traced.0.join("trace"))
            .unwrap()
            .lines()
            .map(str::to_owned)
            .collect::<Vec<_>>();
        assert_eq!(points.len(), 12);
        for point in points {
            for variable in ["ULMDB_H5_FAIL_AT", "ULMDB_H5_EXIT_AT"] {
                let d = Dir::new(base);
                let output = child(&d.0, Some((variable, &point)), None);
                if variable.ends_with("EXIT_AT") {
                    assert_eq!(output.status.code(), Some(86), "{point}");
                } else {
                    assert!(
                        output.status.success(),
                        "{point}: {}",
                        String::from_utf8_lossy(&output.stdout)
                    );
                }
                assert_eq!(
                    fs::read(d.0.join("WAL")).unwrap(),
                    fs::read(d.0.join("wal.before")).unwrap()
                );
                let (j, _) = Journal::open_checked(&d.0.join("WAL"), |p| {
                    assert_eq!(p.records.len(), 1);
                    Ok(())
                })
                .unwrap();
                assert_eq!(j.selected().lsn, 1);
                drop(j);
                if d.0.join("artifact").exists() {
                    // A process cut may leave a valid complete *unselected* artifact
                    // or partial residue. Neither is a WAL-selection fallback.
                    let _ = checkpoint::verify_artifact(
                        &d.0.join("artifact"),
                        CheckpointLimits::default(),
                    );
                    assert!(fs::create_dir(d.0.join("artifact")).is_err());
                }
            }
        }
    }
}

#[test]
fn h0_checkpoint_origins_keep_zero_creation_lsn_losslessly() {
    let mut a = ActiveProvenance::new(identity(1)).unwrap();
    let mut o = origin(1, 4096, 1);
    o.receipt.creation_lsn = 0;
    o.receipt.creation_sequence = 0;
    o.receipt.authority = Authority::Checkpoint {
        generation: 77,
        sequence: 0,
        digest: [12; 32],
    };
    a.insert(o).unwrap();
    let t = a.seal(4192, 0, 0).unwrap();
    let mut bytes = Vec::new();
    provenance::encode(&t, &mut bytes).unwrap();
    let d = provenance::decode(&bytes).unwrap();
    assert_eq!(d.get(o.extent.at), Some(&o));
    assert_eq!(d.bounds().oldest_wal_lsn, 0);
}

#[test]
fn independently_packed_python_sha_vector_fixes_origin_layout() {
    // Python hashlib.sha256(domain + struct.pack fields): common header256,
    // creation(1,1,1001,[1;32])64, extent(4096,96,0,0)16, footer32.
    // No Rust codec was used to calculate this expected vector.
    let mut bytes = Vec::new();
    let stats = provenance::encode(&table(1, 1), &mut bytes).unwrap();
    assert_eq!(bytes.len(), 368);
    assert_eq!(
        stats.digest,
        [
            125, 16, 44, 30, 150, 25, 178, 203, 38, 74, 99, 148, 112, 186, 139, 212, 210, 114, 202,
            123, 61, 124, 89, 167, 149, 90, 85, 215, 31, 111, 193, 75
        ]
    );
    assert_eq!(
        &bytes[320..336],
        &[0, 16, 0, 0, 96, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0]
    );
}

#[test]
fn append_rollback_restores_bounds_and_stale_tokens_cannot_survive_an_aba() {
    let mut a = ActiveProvenance::new(identity(1)).unwrap();
    let outer = a.append_mark();
    a.insert(origin(1, 4096, 1)).unwrap();
    let stale = a.append_mark();
    a.insert(origin(1, 4192, 2)).unwrap();
    a.rollback_append(outer).unwrap();
    assert_eq!(a.len(), 0);
    a.insert(origin(1, 4096, 99)).unwrap();
    assert!(a.rollback_append(stale).is_err());
    assert!(a.seal(4192, 1, 1).is_err());
    let mut a = ActiveProvenance::new(identity(1)).unwrap();
    a.insert(origin(1, 4096, 1)).unwrap();
    let mark = a.append_mark();
    a.insert(origin(1, 4192, 2)).unwrap();
    a.rollback_append(mark).unwrap();
    let t = a.seal(4192, 1, 1).unwrap();
    assert_eq!(t.bounds().last_creation_lsn, 1);
    assert_eq!(t.len(), 1);
    let a = ActiveProvenance::new(identity(1)).unwrap();
    let mut foreign = ActiveProvenance::new(identity(1)).unwrap();
    assert!(foreign.rollback_append(a.append_mark()).is_err());
}
