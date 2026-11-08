//! Common M3 wire/lease witnesses. Linux execution exercises real M3 files and
//! kernel range leases; it makes no claim about a Windows kernel execution.
use super::*;
struct Dir(PathBuf);
impl Drop for Dir {
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.0);
    }
}
struct Fixture {
    region: Arc<Reservation>,
    writer: crate::WriterFence,
    manifest: crate::Manifest,
    state: State,
    dir: Dir,
}
impl Fixture {
    fn new() -> Self {
        Self::with_manifest(crate::Manifest::empty().unwrap())
    }
    fn with_manifest(manifest: crate::Manifest) -> Self {
        let dir =
            Dir(std::env::temp_dir().join(format!("fixed-m3-{}", crate::new_identity().unwrap())));
        crate::create_layout(&dir.0).unwrap();
        let writer =
            crate::WriterFence::try_lock(host::open_regular(&dir.0.join("LOCK"), true).unwrap())
                .unwrap();
        crate::publish(&dir.0, &manifest).unwrap();
        let region = Reservation::reserve(None, 256 << 10).unwrap();
        let p = page().unwrap();
        // These are unselected placement rows, not invented serving graphs.
        let entry = SegmentEntry {
            id: 7,
            cohort: 0,
            length: (2 * p) as u64,
            hash: [0x42; 32],
        };
        let state = State {
            reusable: true,
            version: 3,
            lock_identity: Some(reuse::create_identity(&dir.0, 3).unwrap()),
            next: 2 * p,
            slots: BTreeMap::from([(
                key(&entry),
                Slot {
                    offset: 0,
                    span: 2 * p,
                },
            )]),
            source: None,
        };
        Self {
            region,
            writer,
            manifest,
            state,
            dir,
        }
    }
    fn image(&self) -> Vec<u8> {
        FixedMap::encode_parts(
            self.manifest.uuid,
            self.manifest.epoch,
            self.region.address(),
            self.region.len(),
            &self.state,
        )
        .unwrap()
    }
    fn install(&self, bytes: &[u8]) {
        fs::write(self.dir.0.join("FIXEDMAP"), bytes).unwrap();
    }
    fn decode(&self) -> Result<(usize, usize, State)> {
        FixedMap::decode(&self.dir.0, self.manifest.uuid, self.manifest.epoch)
    }
}
#[test]
fn real_m3_dedicated_identity_roundtrip_and_independent_slot_leases() {
    let f = Fixture::new();
    let bytes = f.image();
    f.install(&bytes);
    assert_eq!(&bytes[..8], MAGIC_V3);
    assert_eq!(bytes.len(), WIDE_HEADER + ROW);
    let id = f.state.lock_identity.unwrap();
    assert_eq!(
        u128::from_le_bytes(bytes[88..104].try_into().unwrap()),
        id.file_id
    );
    let (address, capacity, state) = f.decode().unwrap();
    assert_eq!((address, capacity), (f.region.address(), f.region.len()));
    assert!(state.slots == f.state.slots && state.lock_identity == Some(id));
    let reopened = FixedMapDeclaration::open(&f.dir.0, f.manifest.uuid, f.manifest.epoch).unwrap();
    assert!(reopened.is_reusable().unwrap());
    let p = page().unwrap();
    let a = reuse::lease(&f.dir.0, id, 0, p, false).unwrap();
    let b = reuse::lease(&f.dir.0, id, 0, p, false).unwrap();
    assert!(matches!(
        reuse::lease(&f.dir.0, id, 0, p, true),
        Err(Error::Busy)
    ));
    drop(a);
    assert!(matches!(
        reuse::lease(&f.dir.0, id, 0, p, true),
        Err(Error::Busy)
    ));
    // A nonoverlapping slot remains available; the native writer is held too.
    drop(reuse::lease(&f.dir.0, id, p, p, true).unwrap());
    drop(b);
    drop(reuse::lease(&f.dir.0, id, 0, p, true).unwrap());
    let mut forged = bytes.clone();
    forged[88..104].copy_from_slice(&(id.file_id ^ (1 << 100)).to_le_bytes());
    let mut hash = Sha256::new();
    hash.update(&forged[..128]);
    hash.update(&forged[160..]);
    forged[128..160].copy_from_slice(&hash.finalize());
    f.install(&forged);
    // Recomputed SHA makes this a real full-identity binding negative.
    assert!(FixedMap::decode_image(&f.dir.0, f.manifest.uuid, f.manifest.epoch, false).is_ok());
    assert!(matches!(
        f.decode(),
        Err(Error::Corrupt("fixed slot lease namespace changed"))
    ));
    f.install(&bytes);
    assert!(f.decode().is_ok());
}
#[test]
fn m3_fresh_export_and_owned_bootstrap_bind_target_local_lease() {
    let source = Fixture::new();
    let original = source.image();
    source.install(&original);
    let target = Fixture::with_manifest(source.manifest.clone());
    // Genuine empty roots make these codec fixtures independently valid without
    // treating the unselected placement rows as user-data authority.
    FixedMap::export_layout(
        &source.dir.0,
        &target.dir.0,
        source.manifest.uuid,
        source.manifest.epoch,
        &source.manifest,
        false,
    )
    .unwrap();
    let (_, _, copied) =
        FixedMap::decode(&target.dir.0, source.manifest.uuid, source.manifest.epoch).unwrap();
    assert!(copied.lock_identity != source.state.lock_identity);
    assert!(copied.slots == source.state.slots);
    // Import begins with the source image, whose lease is absent in the target.
    target.install(&original);
    assert!(FixedMap::decode(&target.dir.0, source.manifest.uuid, source.manifest.epoch).is_err());
    let held = host::directory(&target.dir.0).unwrap();
    FixedMap::rebind_imported_layout(
        &target.dir.0,
        &[&source.manifest],
        Some(&target.writer),
        || host::check_directory(&target.dir.0, &held),
    )
    .unwrap();
    let (_, _, imported) =
        FixedMap::decode(&target.dir.0, source.manifest.uuid, source.manifest.epoch).unwrap();
    assert!(imported.lock_identity != source.state.lock_identity);
    assert!(imported.slots == source.state.slots);
    assert_eq!(fs::read(source.dir.0.join("FIXEDMAP")).unwrap(), original);
}
#[cfg(target_os = "linux")]
#[test]
fn legacy_m1_m2_wire_fixture_bytes_remain_exact() {
    assert_eq!(page().unwrap(), 4096);
    let entry = SegmentEntry {
        id: 7,
        cohort: 0,
        length: 8192,
        hash: [0x42; 32],
    };
    for (version, expected) in [
        (
            1,
            "d878e2313c89b8af4ae7b03a306a157b833b35930d8a719036dbd85c68a77ad7",
        ),
        (
            2,
            "1ec154f04e62238f20492e26ab12d988390db96c9f60c3557e0fbd07c39e69ab",
        ),
    ] {
        let state = State {
            version,
            reusable: version == 2,
            lock_identity: (version == 2).then_some(reuse::LeaseIdentity {
                volume: 0x0102030405060708,
                file_id: 0x1112131415161718,
                kind: reuse::LeaseKind::NativeLock,
            }),
            next: 8192,
            slots: BTreeMap::from([(
                key(&entry),
                Slot {
                    offset: 0,
                    span: 8192,
                },
            )]),
            source: None,
        };
        let b = FixedMap::encode_parts([0x11; 16], 4, 0x10000000, 65536, &state).unwrap();
        assert_eq!(b.len(), HEADER + ROW);
        assert_eq!(format!("{:x}", Sha256::digest(b)), expected);
    }
}
