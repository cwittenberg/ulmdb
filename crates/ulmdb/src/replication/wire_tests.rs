use super::*;
use crate::{Environment, replication::receive};
use std::{fs, path::PathBuf};

struct Directory(PathBuf);
impl Directory {
    fn new() -> Self {
        let path = std::env::temp_dir().join(format!(
            "ulmdb-wire-unit-{:032x}",
            ulmdb_storage::new_identity().unwrap()
        ));
        fs::create_dir(&path).unwrap();
        Self(path)
    }
    fn empty(&self) {
        assert_eq!(fs::read_dir(&self.0).unwrap().count(), 0);
    }
}
impl Drop for Directory {
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.0);
    }
}
fn fixture(dir: &Directory) -> (PublisherKey, TransferHeader) {
    let env = Environment::create(dir.0.join("source")).unwrap();
    let cursor = Cursor::from_read(&env.begin_read()).unwrap();
    (
        PublisherKey::new(17, cursor.uuid, cursor.epoch, [37; 32]).unwrap(),
        TransferHeader {
            base: None,
            target: cursor,
            key_id: 17,
            nonce: 1,
            file_count: 0,
            payload_bytes: 0,
        },
    )
}
fn signed(
    key: &PublisherKey,
    header: &[u8; HEADER_BYTES],
    assets: &[(AssetDescription, Vec<u8>)],
) -> Vec<u8> {
    let mut bytes = header.to_vec();
    for (asset, payload) in assets {
        bytes.extend_from_slice(&encode_asset(*asset));
        bytes.extend_from_slice(payload);
    }
    resign(key, &mut bytes);
    bytes
}
fn resign(key: &PublisherKey, bytes: &mut Vec<u8>) {
    let mut mac = authenticator(key).unwrap();
    mac.update(bytes);
    bytes.extend_from_slice(&mac.finalize().into_bytes());
}
#[test]
fn signed_duplicate_and_reordered_assets_are_rejected_and_only_owned_stage_is_removed() {
    let dir = Directory::new();
    let stage = Directory::new();
    let (key, mut header) = fixture(&dir);
    fs::write(stage.0.join("sentinel"), b"keep").unwrap();
    let a = AssetDescription {
        id: AssetId::Descriptor(1),
        length: 192,
    };
    let b = AssetDescription {
        id: AssetId::Descriptor(2),
        length: 192,
    };
    header.file_count = 2;
    header.payload_bytes = 384;
    for ids in [[a, a], [b, a]] {
        let bytes = signed(
            &key,
            &encode(&header),
            &[(ids[0], vec![0; 192]), (ids[1], vec![0; 192])],
        );
        assert!(matches!(
            receive(
                &mut bytes.as_slice(),
                &key,
                TransferLimits::default(),
                &stage.0
            ),
            Err(ReplicationError::Corrupt(_))
        ));
        assert_eq!(fs::read_dir(&stage.0).unwrap().count(), 1);
    }
    assert_eq!(fs::read(stage.0.join("sentinel")).unwrap(), b"keep");
}
#[test]
fn signed_metadata_reserved_fields_lengths_totals_and_header_flags_are_checked() {
    let dir = Directory::new();
    let stage = Directory::new();
    let (key, mut header) = fixture(&dir);
    header.file_count = 1;
    header.payload_bytes = 192;
    let asset = AssetDescription {
        id: AssetId::Descriptor(7),
        length: 192,
    };
    let original = signed(&key, &encode(&header), &[(asset, vec![0; 192])]);
    for offset in [
        12,
        40,
        268,
        280,
        319,
        HEADER_BYTES + 1,
        HEADER_BYTES + 24,
        HEADER_BYTES + 48,
    ] {
        let mut bytes = original[..original.len() - TAG_BYTES].to_vec();
        bytes[offset] ^= 2;
        resign(&key, &mut bytes);
        assert!(
            receive(
                &mut bytes.as_slice(),
                &key,
                TransferLimits::default(),
                &stage.0
            )
            .is_err(),
            "offset {offset}"
        );
        stage.empty();
    }
    for length in [0, 191, 67_109_057, u64::MAX] {
        let mut bytes = original[..original.len() - TAG_BYTES].to_vec();
        bytes[HEADER_BYTES + 40..HEADER_BYTES + 48].copy_from_slice(&length.to_le_bytes());
        resign(&key, &mut bytes);
        assert!(matches!(
            receive(
                &mut bytes.as_slice(),
                &key,
                TransferLimits::default(),
                &stage.0
            ),
            Err(ReplicationError::Corrupt(_))
        ));
        stage.empty();
    }
    let mut bytes = original[..original.len() - TAG_BYTES].to_vec();
    bytes[272..280].copy_from_slice(&193u64.to_le_bytes());
    resign(&key, &mut bytes);
    assert!(matches!(
        receive(
            &mut bytes.as_slice(),
            &key,
            TransferLimits::default(),
            &stage.0
        ),
        Err(ReplicationError::Corrupt(_))
    ));
    stage.empty();
    let mut bytes = original[..original.len() - TAG_BYTES].to_vec();
    bytes[264..268].copy_from_slice(&u32::MAX.to_le_bytes());
    resign(&key, &mut bytes);
    assert!(matches!(
        receive(
            &mut bytes.as_slice(),
            &key,
            TransferLimits::default(),
            &stage.0
        ),
        Err(ReplicationError::LimitExceeded)
    ));
    stage.empty();
}
#[test]
fn binding_failure_precedes_staging_parent_access_and_limits_precede_asset_reads() {
    let dir = Directory::new();
    let (key, mut header) = fixture(&dir);
    let nonexistent = dir.0.join("does-not-exist");
    let wrong = PublisherKey::new(18, key.uuid(), key.epoch(), [37; 32]).unwrap();
    let bytes = signed(&key, &encode(&header), &[]);
    assert!(matches!(
        receive(
            &mut bytes.as_slice(),
            &wrong,
            TransferLimits::default(),
            &nonexistent
        ),
        Err(ReplicationError::UnauthorizedPublisher)
    ));
    header.file_count = 1;
    header.payload_bytes = 192;
    let bytes = encode(&header);
    assert!(matches!(
        receive(
            &mut bytes.as_slice(),
            &key,
            TransferLimits {
                max_files: 0,
                max_bytes: 192
            },
            &nonexistent
        ),
        Err(ReplicationError::LimitExceeded)
    ));
    assert!(!nonexistent.exists());
}
#[test]
fn physical_delta_at_maximum_sequence_and_cursor_crc_are_checked() {
    let dir = Directory::new();
    let (key, mut header) = fixture(&dir);
    let env = Environment::open(dir.0.join("source")).unwrap();
    let mut base = header.target;
    base.sequence = u64::MAX;
    env.upgrade_publication().unwrap();
    let mut target = Cursor::from_read(&env.begin_read()).unwrap();
    target.sequence = u64::MAX;
    header.base = Some(base);
    header.target = target;
    assert!(decode(&encode(&header), &key, TransferLimits::default()).is_ok());
    header.target.sequence = 0;
    assert!(matches!(
        decode(&encode(&header), &key, TransferLimits::default()),
        Err(ReplicationError::Gap)
    ));
    header.target = target;
    header.target.checkpoint[32] ^= 1;
    assert!(decode(&encode(&header), &key, TransferLimits::default()).is_err());
}
#[test]
fn asset_codec_round_trips_and_generations_cannot_escape_canonical_directories() {
    for asset in [
        AssetDescription {
            id: AssetId::Segment([255; 32]),
            length: 4096,
        },
        AssetDescription {
            id: AssetId::Descriptor(u128::MAX),
            length: 192,
        },
        AssetDescription {
            id: AssetId::Cohort(1),
            length: 224,
        },
    ] {
        assert_eq!(decode_asset(&encode_asset(asset)).unwrap(), asset);
        let path = asset.id.path(std::path::Path::new("base"));
        assert_eq!(path.components().count(), 3);
    }
    for zero in [AssetId::Descriptor(0), AssetId::Cohort(0)] {
        assert!(
            decode_asset(&encode_asset(AssetDescription {
                id: zero,
                length: 224
            }))
            .is_err()
        );
    }
    assert!(wire_bytes(u32::MAX, u64::MAX).is_err());
}

#[test]
fn frame_hmac_has_an_independent_known_answer_for_its_exact_domain() {
    // Python stdlib hmac.new(bytes([37])*32,
    // b"ULMDB/replication/frame/v1\0" + bytes(320), hashlib.sha256).
    // The fixed answer detects a coordinated sender/receiver domain change.
    let key = PublisherKey::new(1, [1; 16], 1, [37; 32]).unwrap();
    let expected = [
        0x39, 0x50, 0x1f, 0x0b, 0xc6, 0x83, 0xaa, 0xf9, 0x2c, 0x14, 0x7a, 0x21, 0x78, 0xee, 0x8a,
        0xce, 0x7f, 0x85, 0x21, 0x73, 0x6b, 0x4f, 0x1c, 0xb1, 0xd5, 0xf2, 0x32, 0x74, 0x72, 0xc8,
        0xb9, 0x2b,
    ];
    let mut mac = authenticator(&key).unwrap();
    mac.update(&[0; 320]);
    mac.verify_slice(&expected).unwrap();
}
