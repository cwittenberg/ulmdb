use super::{
    ApplyMode, Cursor, Event, Limits, OriginCheckpoint, Versions, baseline, store, wire::*,
};
use sha2::{Digest, Sha256};
use std::{
    collections::BTreeMap,
    fs::{self, File, OpenOptions},
    io::{Read, Write},
    path::Path,
};
use ulmdb::{Environment, Error, ReadTxn, Result, WriteTxn, backup};

mod controlled;
pub use controlled::{
    export_snapshot_job, export_snapshot_with_control, verify_snapshot_job,
    verify_snapshot_with_control,
};

const MAGIC: &[u8; 8] = b"IPDJNL01";
const BASELINE_MAGIC: &[u8; 8] = b"IPDJNL02";
const MANIFEST: &str = "SNAPSHOT";
const MAX_MANIFEST: usize = 1024 * 1024;

/// A receipt is minted only by exporting or verifying a complete independent
/// recovery bundle. Fields cannot be assembled from different transactions.
/// Digests prove integrity, not publisher authority: authenticate distribution.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct SnapshotReceipt {
    pub(super) realm: u128,
    pub(super) cursors: BTreeMap<u64, Cursor>,
    pub(super) heads: BTreeMap<u64, [u8; 32]>,
    versions: Versions,
    baseline: Option<baseline::Baseline>,
    native: backup::Summary,
    id: [u8; 32],
}
impl SnapshotReceipt {
    pub fn id(&self) -> [u8; 32] {
        self.id
    }
    pub fn native(&self) -> &backup::Summary {
        &self.native
    }
    pub fn versions(&self) -> Versions {
        self.versions
    }
    pub fn cursors(&self) -> &BTreeMap<u64, Cursor> {
        &self.cursors
    }
    pub fn realm(&self) -> u128 {
        self.realm
    }
    /// Read-only cumulative checkpoints at each exported applied position.
    /// Iteration borrows the receipt and allocates no checkpoint collection.
    pub fn checkpoints(&self) -> impl ExactSizeIterator<Item = OriginCheckpoint> + '_ {
        self.cursors
            .iter()
            .map(|(&origin, cursor)| OriginCheckpoint {
                origin,
                sequence: cursor.applied,
                digest: self.heads[&origin],
            })
    }
    pub fn baseline(&self) -> Option<&baseline::Baseline> {
        self.baseline.as_ref()
    }
}

fn heads(read: &ReadTxn, cursors: &BTreeMap<u64, Cursor>) -> Result<BTreeMap<u64, [u8; 32]>> {
    let mut result = BTreeMap::new();
    for (&origin, c) in cursors {
        if c.applied == 0 {
            return Err(Error::Corrupt("zero persisted origin cursor"));
        }
        let b = read
            .get(HEADS, &origin.to_be_bytes())?
            .ok_or(Error::Corrupt("missing journal head digest"))?;
        let digest = b
            .try_into()
            .map_err(|_| Error::Corrupt("journal head digest length"))?;
        if store::chain_at(read, origin, c.applied, *c)? != digest {
            return Err(Error::Corrupt("journal head/cursor chain mismatch"));
        }
        if c.floor > 0 {
            store::chain_at(read, origin, c.floor, *c)?;
        }
        result.insert(origin, digest);
    }
    Ok(result)
}
fn body(receipt: &SnapshotReceipt) -> Result<Vec<u8>> {
    let mut b = Vec::new();
    b.extend_from_slice(if receipt.baseline.is_some() {
        BASELINE_MAGIC
    } else {
        MAGIC
    });
    b.extend_from_slice(&receipt.realm.to_be_bytes());
    b.extend_from_slice(&encode_versions(receipt.versions)?);
    let n = &receipt.native;
    b.extend_from_slice(&n.uuid);
    b.extend_from_slice(&n.generation.to_be_bytes());
    b.extend_from_slice(&n.sequence.to_be_bytes());
    b.extend_from_slice(&n.commit.to_be_bytes());
    b.extend_from_slice(&(n.segments as u64).to_be_bytes());
    b.extend_from_slice(&n.segment_bytes.to_be_bytes());
    b.extend_from_slice(&n.physical_bytes.to_be_bytes());
    b.extend_from_slice(&(receipt.cursors.len() as u32).to_be_bytes());
    for (&origin, &cursor) in &receipt.cursors {
        b.extend_from_slice(&origin.to_be_bytes());
        b.extend_from_slice(&encode_cursor(cursor));
        b.extend_from_slice(
            receipt
                .heads
                .get(&origin)
                .ok_or(Error::Corrupt("missing snapshot origin digest"))?,
        );
    }
    if let Some(baseline) = &receipt.baseline {
        let bytes = baseline::encode_baseline(baseline)?;
        b.extend_from_slice(
            &u32::try_from(bytes.len())
                .map_err(|_| Error::LimitExceeded)?
                .to_be_bytes(),
        );
        b.extend_from_slice(&bytes);
    }
    Ok(b)
}
fn write_manifest(target: &Path, receipt: &mut SnapshotReceipt) -> Result<()> {
    let mut bytes = body(receipt)?;
    receipt.id = Sha256::digest(&bytes).into();
    bytes.extend_from_slice(&receipt.id);
    let mut output = OpenOptions::new()
        .write(true)
        .create_new(true)
        .open(target.join(MANIFEST))?;
    output.write_all(&bytes)?;
    output.sync_all()?;
    File::open(target)?.sync_all()?;
    Ok(())
}

/// Export exactly `read`, including all origins and application version metadata.
/// `target` must not exist. Its completion marker is published only after the
/// native copy validates and all manifest bytes are synchronized. A failed export
/// can leave an incomplete target; verification always rejects it.
pub fn export_snapshot(
    environment: &Environment,
    read: &ReadTxn,
    target: impl AsRef<Path>,
) -> Result<SnapshotReceipt> {
    let target = target.as_ref();
    let realm = store::realm(read)?;
    let cursors = store::cursors(read)?;
    let heads = heads(read, &cursors)?;
    let versions = store::versions(read)?;
    let baseline = baseline::provenance(read)?;
    fs::create_dir(target)?;
    let native = backup::export_snapshot(environment, read, target.join("database"))?;
    let mut receipt = SnapshotReceipt {
        realm,
        cursors,
        heads,
        versions,
        baseline,
        native,
        id: [0; 32],
    };
    write_manifest(target, &mut receipt)?;
    if let Some(parent) = target.parent() {
        File::open(if parent.as_os_str().is_empty() {
            Path::new(".")
        } else {
            parent
        })?
        .sync_all()?;
    }
    Ok(receipt)
}

struct Decoder<'a>(&'a [u8]);
impl<'a> Decoder<'a> {
    fn take(&mut self, n: usize) -> Result<&'a [u8]> {
        if self.0.len() < n {
            return Err(Error::Corrupt("truncated journal snapshot manifest"));
        }
        let (a, b) = self.0.split_at(n);
        self.0 = b;
        Ok(a)
    }
    fn u64(&mut self) -> Result<u64> {
        number(self.take(8)?)
    }
    fn u128(&mut self) -> Result<u128> {
        Ok(u128::from_be_bytes(self.take(16)?.try_into().unwrap()))
    }
}
fn read_manifest(target: &Path) -> Result<SnapshotReceipt> {
    for directory in [target.to_path_buf(), target.join("database")] {
        if !fs::symlink_metadata(directory)?.file_type().is_dir() {
            return Err(Error::Corrupt("snapshot directory type"));
        }
    }
    let path = target.join(MANIFEST);
    let metadata = fs::symlink_metadata(&path)?;
    if !metadata.file_type().is_file() || metadata.len() > MAX_MANIFEST as u64 {
        return Err(Error::Corrupt("snapshot manifest type or size"));
    }
    let mut bytes = Vec::new();
    File::open(path)?
        .take((MAX_MANIFEST + 1) as u64)
        .read_to_end(&mut bytes)?;
    if bytes.len() < 32 || bytes.len() > MAX_MANIFEST {
        return Err(Error::Corrupt("snapshot manifest length"));
    }
    let at = bytes.len() - 32;
    let id: [u8; 32] = Sha256::digest(&bytes[..at]).into();
    decode_manifest(&bytes, id)
}

fn decode_manifest(bytes: &[u8], id: [u8; 32]) -> Result<SnapshotReceipt> {
    if bytes.len() < 32 || bytes.len() > MAX_MANIFEST {
        return Err(Error::Corrupt("snapshot manifest length"));
    }
    let at = bytes.len() - 32;
    if id.as_slice() != &bytes[at..] {
        return Err(Error::Corrupt("snapshot manifest digest"));
    }
    let mut d = Decoder(&bytes[..at]);
    let magic = d.take(8)?;
    if magic != MAGIC && magic != BASELINE_MAGIC {
        return Err(Error::Corrupt("snapshot manifest magic"));
    }
    let realm = d.u128()?;
    if realm == 0 {
        return Err(Error::Corrupt("snapshot realm"));
    }
    let versions = decode_versions(d.take(16)?)?;
    let native = backup::Summary {
        uuid: d.take(16)?.try_into().unwrap(),
        generation: d.u128()?,
        sequence: d.u64()?,
        commit: d.u128()?,
        segments: usize::try_from(d.u64()?).map_err(|_| Error::LimitExceeded)?,
        segment_bytes: d.u64()?,
        physical_bytes: d.u64()?,
    };
    let count = u32::from_be_bytes(d.take(4)?.try_into().unwrap()) as usize;
    if count > MAX_ORIGINS {
        return Err(Error::Corrupt("snapshot origin count"));
    }
    let mut cursors = BTreeMap::new();
    let mut heads = BTreeMap::new();
    let mut previous = 0;
    for _ in 0..count {
        let origin = d.u64()?;
        let cursor = decode_cursor(d.take(24)?)?;
        let hash = d.take(32)?.try_into().unwrap();
        if origin <= previous || cursor.applied == 0 {
            return Err(Error::Corrupt("snapshot origin ordering/cursor"));
        }
        previous = origin;
        cursors.insert(origin, cursor);
        heads.insert(origin, hash);
    }
    let baseline = if magic == BASELINE_MAGIC {
        let length = u32::from_be_bytes(d.take(4)?.try_into().unwrap()) as usize;
        Some(baseline::decode_baseline(d.take(length)?)?)
    } else {
        None
    };
    if baseline.as_ref().is_some_and(|b| b.realm != realm) {
        return Err(Error::Corrupt("snapshot baseline realm differs"));
    }
    if !d.0.is_empty() {
        return Err(Error::Corrupt("snapshot manifest trailing data"));
    }
    Ok(SnapshotReceipt {
        realm,
        cursors,
        heads,
        versions,
        baseline,
        native,
        id,
    })
}
fn verify_environment(environment: &Environment, receipt: &SnapshotReceipt) -> Result<()> {
    let read = environment.begin_read();
    let cursors = store::cursors(&read)?;
    if backup::verify(environment)? != receipt.native
        || store::realm(&read)? != receipt.realm
        || store::versions(&read)? != receipt.versions
        || baseline::provenance(&read)? != receipt.baseline
        || cursors != receipt.cursors
        || heads(&read, &cursors)? != receipt.heads
    {
        return Err(Error::Corrupt(
            "snapshot metadata does not match native commit",
        ));
    }
    Ok(())
}

pub fn verify_snapshot(target: impl AsRef<Path>) -> Result<SnapshotReceipt> {
    let target = target.as_ref();
    let receipt = read_manifest(target)?;
    let environment = Environment::open(target.join("database"))?;
    verify_environment(&environment, &receipt)?;
    Ok(receipt)
}

/// Restore into a new native directory. The previous live environment is never
/// modified. Caller selects/activates the returned environment after admission.
pub fn restore_snapshot(source: impl AsRef<Path>, target: impl AsRef<Path>) -> Result<Environment> {
    let source = source.as_ref();
    let receipt = read_manifest(source)?;
    // Hold the exact source environment open through validation and copying so
    // verification cannot be separated from a substituted CURRENT.
    let environment = Environment::open(source.join("database"))?;
    verify_environment(&environment, &receipt)?;
    let read = environment.begin_read();
    backup::export_snapshot(&environment, &read, target.as_ref())?;
    let restored = Environment::open(target)?;
    verify_environment(&restored, &receipt)?;
    Ok(restored)
}

/// Bootstrap an existing region while preserving its designated local origin.
/// The old environment stays live and untouched. The caller must fence its local
/// writer during final handover; events written after this function captures its
/// read snapshot are intentionally outside this candidate and require another
/// catch-up before activation. A missing/pruned local tail fails explicitly.
pub fn rebase_snapshot<F>(
    source: impl AsRef<Path>,
    local: &Environment,
    local_origin: u64,
    target: impl AsRef<Path>,
    limits: Limits,
    mut apply: F,
) -> Result<Environment>
where
    F: FnMut(&mut WriteTxn<'_>, &Event) -> Result<()>,
{
    limit(limits)?;
    let read = local.begin_read();
    let local_cursor = store::cursor(&read, local_origin)?;
    let receipt = verify_snapshot(source.as_ref())?;
    if receipt.realm != store::realm(&read)? {
        return Err(Error::InvalidInput(
            "snapshot belongs to another journal realm",
        ));
    }
    if receipt.versions != store::versions(&read)? {
        return Err(Error::InvalidInput("snapshot application versions differ"));
    }
    if receipt.baseline != baseline::provenance(&read)? {
        return Err(Error::InvalidInput("snapshot application baseline differs"));
    }
    let remote = receipt
        .cursors
        .get(&local_origin)
        .copied()
        .unwrap_or_default();
    if remote.applied > local_cursor.applied {
        return Err(Error::InvalidInput(
            "snapshot is ahead of local origin; recovery authority required",
        ));
    }
    if remote.applied < local_cursor.floor {
        return Err(Error::InvalidInput(
            "local origin tail pruned; newer snapshot required",
        ));
    }
    if remote.applied < local_cursor.applied && remote.serving != remote.applied {
        return Err(Error::InvalidInput(
            "snapshot has unmaterialized local origin events",
        ));
    }
    if remote.applied == local_cursor.applied && remote.serving < local_cursor.serving {
        return Err(Error::InvalidInput(
            "snapshot serving watermark regresses local origin",
        ));
    }
    if remote.applied > 0 {
        let digest = store::chain_at(&read, local_origin, remote.applied, local_cursor)?;
        if receipt.heads.get(&local_origin) != Some(&digest) {
            return Err(Error::InvalidInput(
                "snapshot conflicts with local origin lineage",
            ));
        }
    }
    let restored = restore_snapshot(source, target)?;
    verify_environment(&restored, &receipt)?;
    let mut after = remote.applied;
    while after < local_cursor.applied {
        let batch = store::page(&read, local_origin, after, limits)?;
        let mut write = restored.begin_write()?;
        store::apply_page(
            &mut write,
            &batch.events,
            limits,
            ApplyMode::Synchronous,
            |txn, event| apply(txn, event),
        )?;
        after = batch
            .events
            .last()
            .ok_or(Error::Corrupt("empty local recovery page"))?
            .sequence;
        write.commit()?;
    }
    Ok(restored)
}

/// Rebase every locally known origin, preserving imported observations as well
/// as the designated collector's own tail. A retained shared chain boundary is
/// required wherever both databases contain an origin. Newer remote origins are
/// accepted, but conflicting history and an unavailable local tail fail closed.
/// Use `validate_full_handover` after catch-up and retain its guard while switching.
pub fn rebase_all_snapshot<F>(
    source: impl AsRef<Path>,
    local: &Environment,
    target: impl AsRef<Path>,
    limits: Limits,
    mut apply: F,
) -> Result<Environment>
where
    F: FnMut(&mut WriteTxn<'_>, &Event) -> Result<()>,
{
    limit(limits)?;
    let read = local.begin_read();
    let local_cursors = store::cursors(&read)?;
    let receipt = verify_snapshot(source.as_ref())?;
    if receipt.realm != store::realm(&read)? {
        return Err(Error::InvalidInput(
            "snapshot belongs to another journal realm",
        ));
    }
    if receipt.versions != store::versions(&read)? {
        return Err(Error::InvalidInput("snapshot application versions differ"));
    }
    if receipt.baseline != baseline::provenance(&read)? {
        return Err(Error::InvalidInput("snapshot application baseline differs"));
    }
    for (&origin, &local_cursor) in &local_cursors {
        let remote = receipt.cursors.get(&origin).copied().unwrap_or_default();
        if remote.applied <= local_cursor.applied {
            if remote.applied < local_cursor.floor {
                return Err(Error::InvalidInput(
                    "local origin tail pruned; newer snapshot required",
                ));
            }
            if remote.applied > 0
                && receipt.heads.get(&origin)
                    != Some(&store::chain_at(
                        &read,
                        origin,
                        remote.applied,
                        local_cursor,
                    )?)
            {
                return Err(Error::InvalidInput(
                    "snapshot conflicts with local origin lineage",
                ));
            }
            if remote.applied < local_cursor.applied && remote.serving != remote.applied {
                return Err(Error::InvalidInput(
                    "snapshot has unmaterialized origin events",
                ));
            }
            if remote.applied == local_cursor.applied && remote.serving < local_cursor.serving {
                return Err(Error::InvalidInput(
                    "snapshot serving watermark regresses local origin",
                ));
            }
        }
    }
    let restored = restore_snapshot(source, target)?;
    verify_environment(&restored, &receipt)?;
    {
        let remote_read = restored.begin_read();
        for (&origin, &local_cursor) in &local_cursors {
            let remote = receipt.cursors.get(&origin).copied().unwrap_or_default();
            if remote.applied > local_cursor.applied
                && (remote.serving < local_cursor.serving
                    || store::chain_at(&remote_read, origin, local_cursor.applied, remote)?
                        != store::chain_at(&read, origin, local_cursor.applied, local_cursor)?)
            {
                return Err(Error::InvalidInput(
                    "newer snapshot conflicts with local origin history",
                ));
            }
        }
    }
    for (&origin, &local_cursor) in &local_cursors {
        let mut after = receipt
            .cursors
            .get(&origin)
            .copied()
            .unwrap_or_default()
            .applied;
        while after < local_cursor.applied {
            let batch = store::page(&read, origin, after, limits)?;
            let mut write = restored.begin_write()?;
            store::apply_page(
                &mut write,
                &batch.events,
                limits,
                ApplyMode::Synchronous,
                |txn, event| apply(txn, event),
            )?;
            after = batch
                .events
                .last()
                .ok_or(Error::Corrupt("empty recovery page"))?
                .sequence;
            write.commit()?;
        }
    }
    Ok(restored)
}

/// Keeps both environments' writer mutexes held after validation. Customer reads
/// continue. Retain this value while atomically switching the application's
/// active environment handle, then release it. Neither fence commits changes.
/// Stop or reroute queued application writes before release: a native fence
/// cannot redirect tasks that already hold an old environment handle.
pub struct HandoverGuard<'a> {
    _local: WriteTxn<'a>,
    _candidate: WriteTxn<'a>,
    local_commit: u128,
    candidate_commit: u128,
}
impl HandoverGuard<'_> {
    pub fn local_commit(&self) -> u128 {
        self.local_commit
    }
    pub fn candidate_commit(&self) -> u128 {
        self.candidate_commit
    }
}

fn handover<'a>(
    local: &'a Environment,
    candidate: &'a Environment,
    local_origin: Option<u64>,
) -> Result<HandoverGuard<'a>> {
    let (local_fence, candidate_fence) = writer_fences(local, candidate)?;
    let source = local.begin_read();
    let target = candidate.begin_read();
    if store::realm(&source)? != store::realm(&target)?
        || store::versions(&source)? != store::versions(&target)?
        || baseline::identity(&source)? != baseline::identity(&target)?
    {
        return Err(Error::InvalidInput(
            "handover realm or application versions differ",
        ));
    }
    let source_cursors = match local_origin {
        Some(origin) => BTreeMap::from([(origin, store::cursor(&source, origin)?)]),
        None => store::cursors(&source)?,
    };
    for (origin, source_cursor) in source_cursors {
        let target_cursor = store::cursor(&target, origin)?;
        if target_cursor.applied < source_cursor.applied
            || target_cursor.serving < source_cursor.serving
            || (local_origin.is_some() && target_cursor.applied != source_cursor.applied)
        {
            return Err(Error::InvalidInput(
                "candidate loses local origin observations or serving progress",
            ));
        }
        if store::chain_at(&target, origin, source_cursor.applied, target_cursor)?
            != store::chain_at(&source, origin, source_cursor.applied, source_cursor)?
        {
            return Err(Error::InvalidInput("handover origin history conflicts"));
        }
    }
    Ok(HandoverGuard {
        _local: local_fence,
        _candidate: candidate_fence,
        local_commit: source.commit_identity(),
        candidate_commit: target.commit_identity(),
    })
}

fn writer_fences<'a>(
    local: &'a Environment,
    candidate: &'a Environment,
) -> Result<(WriteTxn<'a>, WriteTxn<'a>)> {
    if std::ptr::eq(local, candidate) {
        return Err(Error::InvalidInput(
            "handover requires an independent candidate",
        ));
    }
    // One process can safely validate concurrent opposite-direction handovers:
    // acquire native environment mutexes in address order to avoid lock cycles.
    let (local_fence, candidate_fence) =
        if std::ptr::from_ref(local).addr() < std::ptr::from_ref(candidate).addr() {
            let a = local.begin_write()?;
            let b = candidate.begin_write()?;
            (a, b)
        } else {
            let b = candidate.begin_write()?;
            let a = local.begin_write()?;
            (a, b)
        };
    Ok((local_fence, candidate_fence))
}

/// Deliberate cross-realm activation of a newly imported application baseline.
/// Unlike ordinary handover, a mandatory application verifier proves the actual
/// source/candidate state and complete origin vector against baseline provenance.
/// A candidate with any post-baseline tail must first be activated through this
/// cutover; later recovery uses the ordinary same-realm guards.
pub fn validate_baseline_handover<'a, F>(
    local: &'a Environment,
    candidate: &'a Environment,
    verify: F,
) -> super::ProtocolResult<HandoverGuard<'a>>
where
    F: FnOnce(
        &ReadTxn,
        &ReadTxn,
        &baseline::Baseline,
    ) -> super::ProtocolResult<baseline::BaselineActivation>,
{
    use super::ProtocolError;
    let (local_fence, candidate_fence) = writer_fences(local, candidate)?;
    let source = local.begin_read();
    let target = candidate.begin_read();
    let baseline = baseline::provenance(&target)?.ok_or(ProtocolError::BaselineMismatch(
        "candidate has no imported baseline",
    ))?;
    match store::realm(&source) {
        Ok(realm) if realm == baseline.realm => {
            return Err(ProtocolError::BaselineMismatch(
                "baseline cutover requires a different source realm",
            ));
        }
        Ok(_) | Err(Error::MissingDatabase) => {}
        Err(error) => return Err(error.into()),
    }
    if store::versions(&target)? != baseline.versions {
        return Err(ProtocolError::VersionMismatch {
            expected: baseline.versions,
            actual: store::versions(&target)?,
        });
    }
    let cursors = store::cursors(&target)?;
    let expected = baseline
        .origins
        .iter()
        .filter(|(_, seq)| **seq > 0)
        .map(|(&id, &seq)| {
            (
                id,
                Cursor {
                    applied: seq,
                    serving: seq,
                    floor: seq,
                },
            )
        })
        .collect::<BTreeMap<_, _>>();
    if cursors != expected {
        return Err(ProtocolError::BaselineMismatch(
            "candidate has changed its imported cursor vector",
        ));
    }
    let identity = baseline.identity()?;
    for (&origin, &cursor) in &cursors {
        if store::chain_at(&target, origin, cursor.applied, cursor)?
            != baseline::anchor(&identity, origin, cursor.applied)
        {
            return Err(ProtocolError::Fork {
                origin,
                sequence: cursor.applied,
            });
        }
    }
    let activated = verify(&source, &target, &baseline)?;
    if activated.application_digest != baseline.application_digest
        || activated.origins != baseline.origins
    {
        return Err(ProtocolError::BaselineMismatch(
            "application verifier disagrees with cutover baseline",
        ));
    }
    if activated.versions != baseline.versions {
        return Err(ProtocolError::VersionMismatch {
            expected: baseline.versions,
            actual: activated.versions,
        });
    }
    Ok(HandoverGuard {
        _local: local_fence,
        _candidate: candidate_fence,
        local_commit: source.commit_identity(),
        candidate_commit: target.commit_identity(),
    })
}

/// Fence and validate the designated local origin through the latest committed
/// write, catching observations committed after a rebase captured its snapshot.
/// Other origins may regress; prefer `validate_full_handover` for full replicas.
pub fn validate_handover<'a>(
    local: &'a Environment,
    candidate: &'a Environment,
    local_origin: u64,
) -> Result<HandoverGuard<'a>> {
    handover(local, candidate, Some(local_origin))
}

/// Fence and validate that no applied/serving origin position regresses and every
/// shared history prefix agrees. The guard closes the validation/activation race.
pub fn validate_full_handover<'a>(
    local: &'a Environment,
    candidate: &'a Environment,
) -> Result<HandoverGuard<'a>> {
    handover(local, candidate, None)
}
