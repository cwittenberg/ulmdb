//! Explicit cutover from an application baseline into a new history namespace.
//!
//! The journal cannot interpret an arbitrary application's state. The mandatory
//! activation callback must install that state and calculate its digest from the
//! installed data, then return the matching version/cursor manifest. IPDon's typed
//! facade provides a concrete verifier for its own tables. A caller assertion of
//! an unverified digest is not an authenticated or complete application import.
use super::{Cursor, OriginCheckpoint, ProtocolError, ProtocolResult, Versions, store, wire::*};
use sha2::{Digest, Sha256};
use std::{collections::BTreeMap, path::Path};
use ulmdb::{Environment, Error, ReadTxn, Result, WriteTxn};

const BASELINE: &[u8] = b"baseline";
const MAGIC: &[u8; 8] = b"IPDBASE1";
const FIXED: usize = 8 + 16 + 32 + 16 + 8 + 4;

/// Shared, verified cutover metadata. `imported_at` belongs to the shared
/// baseline manifest: every peer uses the same timestamp, rather than its own
/// local installation time. Every configured old origin belongs in `origins`,
/// including positions zero; zero positions remain implicit native cursors.
/// Imported positions must leave room for at least one next sequence.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct Baseline {
    pub realm: u128,
    pub application_digest: [u8; 32],
    pub versions: Versions,
    pub imported_at: u64,
    pub origins: BTreeMap<u64, u64>,
}

/// Produced only after the application has installed and verified its complete
/// baseline state inside the provided transaction. This descriptor is compared
/// with the shared baseline before any anchors become visible.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct BaselineActivation {
    pub application_digest: [u8; 32],
    pub versions: Versions,
    pub origins: BTreeMap<u64, u64>,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct BaselineReceipt {
    baseline: Baseline,
    id: [u8; 32],
}
impl BaselineReceipt {
    pub fn id(&self) -> [u8; 32] {
        self.id
    }
    pub fn baseline(&self) -> &Baseline {
        &self.baseline
    }
    /// Imported positive positions are retained floor checkpoints. A zero
    /// position has the ordinary genesis hash and contributes to baseline ID.
    pub fn checkpoints(&self) -> Vec<OriginCheckpoint> {
        self.baseline
            .origins
            .iter()
            .map(|(&origin, &sequence)| OriginCheckpoint {
                origin,
                sequence,
                digest: anchor(&self.id, origin, sequence),
            })
            .collect()
    }
}

impl Baseline {
    pub fn identity(&self) -> Result<[u8; 32]> {
        Ok(identity_bytes(&encode_baseline(self)?))
    }
}

fn identity_bytes(bytes: &[u8]) -> [u8; 32] {
    let mut hash = Sha256::new();
    hash.update(b"IPDon/application-baseline/v1\0");
    hash.update(bytes);
    hash.finalize().into()
}

pub(super) fn anchor(id: &[u8; 32], origin: u64, sequence: u64) -> [u8; 32] {
    if sequence == 0 {
        return [0; 32];
    }
    let mut hash = Sha256::new();
    hash.update(b"IPDon/application-baseline-origin/v1\0");
    hash.update(id);
    hash.update(origin.to_be_bytes());
    hash.update(sequence.to_be_bytes());
    hash.finalize().into()
}

/// Canonical shared manifest used by snapshot provenance and baseline identity.
pub(crate) fn encode_baseline(baseline: &Baseline) -> Result<Vec<u8>> {
    if baseline.realm == 0 || baseline.application_digest == [0; 32] || baseline.imported_at == 0 {
        return Err(Error::InvalidInput(
            "baseline realm/digest/import time must be nonzero",
        ));
    }
    let versions = encode_versions(baseline.versions)?;
    if baseline.origins.len() > MAX_ORIGINS
        || baseline.origins.keys().any(|origin| *origin == 0)
        || baseline
            .origins
            .values()
            .any(|sequence| *sequence == u64::MAX)
    {
        return Err(Error::InvalidInput(
            "baseline origins exceed limits, contain zero ID or exhaust sequence space",
        ));
    }
    let mut bytes = Vec::with_capacity(FIXED + baseline.origins.len() * 16);
    bytes.extend_from_slice(MAGIC);
    bytes.extend_from_slice(&baseline.realm.to_be_bytes());
    bytes.extend_from_slice(&baseline.application_digest);
    bytes.extend_from_slice(&versions);
    bytes.extend_from_slice(&baseline.imported_at.to_be_bytes());
    bytes.extend_from_slice(&(baseline.origins.len() as u32).to_be_bytes());
    for (&origin, &sequence) in &baseline.origins {
        bytes.extend_from_slice(&origin.to_be_bytes());
        bytes.extend_from_slice(&sequence.to_be_bytes());
    }
    Ok(bytes)
}

pub(crate) fn decode_baseline(bytes: &[u8]) -> Result<Baseline> {
    if bytes.len() < FIXED || &bytes[..8] != MAGIC {
        return Err(Error::Corrupt("baseline header/extent"));
    }
    let count = u32::from_be_bytes(bytes[80..84].try_into().unwrap()) as usize;
    if count > MAX_ORIGINS || bytes.len() != FIXED + count * 16 {
        return Err(Error::Corrupt("baseline origin vector extent"));
    }
    let mut origins = BTreeMap::new();
    let mut previous = 0;
    for record in bytes[FIXED..].chunks_exact(16) {
        let origin = number(&record[..8])?;
        let sequence = number(&record[8..])?;
        if origin <= previous {
            return Err(Error::Corrupt("baseline origin ordering"));
        }
        previous = origin;
        origins.insert(origin, sequence);
    }
    let baseline = Baseline {
        realm: u128::from_be_bytes(bytes[8..24].try_into().unwrap()),
        application_digest: bytes[24..56].try_into().unwrap(),
        versions: decode_versions(&bytes[56..72])?,
        imported_at: number(&bytes[72..80])?,
        origins,
    };
    if encode_baseline(&baseline).map_err(|_| Error::Corrupt("invalid persisted baseline"))?
        != bytes
    {
        return Err(Error::Corrupt("noncanonical baseline"));
    }
    Ok(baseline)
}

pub fn provenance(read: &ReadTxn) -> Result<Option<Baseline>> {
    let realm = store::realm(read)?;
    let baseline = read.get(META, BASELINE)?.map(decode_baseline).transpose()?;
    if baseline.as_ref().is_some_and(|b| b.realm != realm) {
        return Err(Error::Corrupt("baseline realm differs from journal"));
    }
    Ok(baseline)
}

pub fn identity(read: &ReadTxn) -> Result<Option<[u8; 32]>> {
    provenance(read)?.map(|b| b.identity()).transpose()
}

pub(super) fn write_identity(txn: &WriteTxn<'_>) -> Result<Option<[u8; 32]>> {
    let realm = store::realm_write(txn)?;
    let baseline = txn.get(META, BASELINE)?.map(decode_baseline).transpose()?;
    if baseline.as_ref().is_some_and(|b| b.realm != realm) {
        return Err(Error::Corrupt("baseline realm differs from journal"));
    }
    baseline.map(|b| b.identity()).transpose()
}

/// Install one baseline into a journal that has never been initialized. The
/// callback and all journal metadata are protected by one savepoint. Existing
/// application records are permitted only insofar as the callback's complete
/// installed-state verifier includes them in `application_digest`.
///
/// `install` must calculate its activation descriptor from actual installed
/// application state. Do not return the expected descriptor without verification.
/// On failure the caller's surrounding transaction remains usable and unchanged
/// relative to this operation's entry state. The caller commits publication.
pub fn import_baseline<F>(
    txn: &mut WriteTxn<'_>,
    baseline: &Baseline,
    install: F,
) -> ProtocolResult<BaselineReceipt>
where
    F: FnOnce(&mut WriteTxn<'_>) -> Result<BaselineActivation>,
{
    let encoded = encode_baseline(baseline)?;
    match txn.get(META, FORMAT) {
        Err(Error::MissingDatabase) => {}
        Ok(_) => {
            return Err(ProtocolError::BaselineMismatch(
                "journal realm/history already exists",
            ));
        }
        Err(error) => return Err(error.into()),
    }
    let id = identity_bytes(&encoded);
    let mut save = txn.savepoint()?;
    store::initialize_with_realm(&mut save, baseline.realm)?;
    store::set_versions(&mut save, baseline.versions)?;
    let activated = install(&mut save)?;
    if activated.versions != baseline.versions {
        return Err(ProtocolError::VersionMismatch {
            expected: baseline.versions,
            actual: activated.versions,
        });
    }
    if activated.application_digest != baseline.application_digest
        || activated.origins != baseline.origins
    {
        return Err(ProtocolError::BaselineMismatch(
            "application digest or complete origin vector differs",
        ));
    }
    if store::realm_write(&save)? != baseline.realm {
        return Err(ProtocolError::BaselineMismatch(
            "application changed the new realm",
        ));
    }
    let actual_versions = decode_versions(
        save.get(META, VERSIONS)?
            .ok_or(Error::Corrupt("baseline versions missing"))?,
    )?;
    if actual_versions != baseline.versions {
        return Err(ProtocolError::VersionMismatch {
            expected: baseline.versions,
            actual: actual_versions,
        });
    }
    if save.get(META, BASELINE)?.is_some()
        || number(
            save.get(META, b"origin_count")?
                .ok_or(Error::Corrupt("baseline origin count missing"))?,
        )? != 0
    {
        return Err(ProtocolError::BaselineMismatch(
            "application published journal history before activation",
        ));
    }
    let mut positive = 0u64;
    for (&origin, &sequence) in &baseline.origins {
        if sequence == 0 {
            continue;
        }
        let hash = anchor(&id, origin, sequence);
        save.put(
            CURSORS,
            &origin.to_be_bytes(),
            &encode_cursor(Cursor {
                applied: sequence,
                serving: sequence,
                floor: sequence,
            }),
        )?;
        save.put(HEADS, &origin.to_be_bytes(), &hash)?;
        save.put(FLOORS, &origin.to_be_bytes(), &hash)?;
        positive += 1;
    }
    save.put(META, b"origin_count", &positive.to_be_bytes())?;
    save.put(META, BASELINE, &encoded)?;
    save.release()?;
    Ok(BaselineReceipt {
        baseline: baseline.clone(),
        id,
    })
}

/// Build a staged native candidate. The destination must not exist. Failed
/// installation can leave an unactivated directory for the caller to inspect;
/// no existing active environment is changed or implicitly selected.
pub fn create_from_baseline<F>(
    target: impl AsRef<Path>,
    baseline: &Baseline,
    install: F,
) -> ProtocolResult<(Environment, BaselineReceipt)>
where
    F: FnOnce(&mut WriteTxn<'_>) -> Result<BaselineActivation>,
{
    encode_baseline(baseline)?;
    let environment = Environment::create(target)?;
    let mut write = environment.begin_write()?;
    let receipt = import_baseline(&mut write, baseline, install)?;
    write.commit()?;
    Ok((environment, receipt))
}

/// Internal cutover helper used only on a separately copied, unactivated native
/// candidate after the typed facade has verified its application state. Never
/// call this on the active source. It drops exactly the five journal databases;
/// application indexes and evidence stay untouched. There is deliberately no
/// public API for resetting an existing realm or history in place.
pub(crate) fn reset_copied_journal(txn: &mut WriteTxn<'_>) -> Result<()> {
    let mut save = txn.savepoint()?;
    store::realm_write(&save)?;
    for table in [META, EVENTS, CURSORS, HEADS, FLOORS] {
        save.drop_database(table)?;
    }
    save.release()
}

#[cfg(test)]
mod codec_tests {
    use super::*;

    #[test]
    fn canonical_manifest_rejects_truncation_duplicate_origins_and_bad_extent() {
        let baseline = Baseline {
            realm: 9,
            application_digest: [7; 32],
            versions: Versions {
                schema: 1,
                policy: 1,
            },
            imported_at: 8,
            origins: BTreeMap::from([(1, 11), (3, 0)]),
        };
        let bytes = encode_baseline(&baseline).unwrap();
        assert_eq!(decode_baseline(&bytes).unwrap(), baseline);
        for len in 0..bytes.len() {
            assert!(decode_baseline(&bytes[..len]).is_err());
        }
        let mut duplicate = bytes.clone();
        duplicate[FIXED + 16..FIXED + 24].copy_from_slice(&1u64.to_be_bytes());
        assert!(decode_baseline(&duplicate).is_err());
        let mut trailing = bytes.clone();
        trailing.push(0);
        assert!(decode_baseline(&trailing).is_err());
        let mut malformed_count = bytes;
        malformed_count[80..84].copy_from_slice(&u32::MAX.to_be_bytes());
        assert!(decode_baseline(&malformed_count).is_err());
    }
}
