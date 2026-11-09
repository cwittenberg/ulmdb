//! Caller-controlled native backup journeys and the journal completion boundary.
use super::*;
use std::{ops::Bound, sync::Arc};
use ulmdb::backup::{BackupControl, BackupJob};

const BLOCK: usize = 64 * 1024;
const PENDING_MANIFEST: &str = ".SNAPSHOT.pending";
const PAYLOAD_OVERHEAD: usize = EVENT_OVERHEAD - 32;

fn cursors(read: &ReadTxn, control: &BackupControl) -> Result<BTreeMap<u64, Cursor>> {
    control.checkpoint(1, 0)?;
    store::realm(read)?;
    let mut result = BTreeMap::new();
    for item in read.range(CURSORS, Bound::Unbounded, Bound::Unbounded, false)? {
        let (key, bytes) = item?;
        control.checkpoint(1, (key.len() + bytes.len()) as u64)?;
        let origin = number(key)?;
        if origin == 0 || result.len() >= MAX_ORIGINS {
            return Err(Error::Corrupt("journal origin limit/identity"));
        }
        result.insert(origin, decode_cursor(bytes)?);
    }
    Ok(result)
}

fn digest(bytes: &[u8], control: &BackupControl) -> Result<[u8; 32]> {
    let mut digest = Sha256::new();
    for chunk in bytes.chunks(BLOCK) {
        control.checkpoint(1, chunk.len() as u64)?;
        digest.update(chunk);
    }
    Ok(digest.finalize().into())
}

/// Validate a retained chain endpoint without cloning its application payload.
fn chain_at(
    read: &ReadTxn,
    origin: u64,
    sequence: u64,
    cursor: Cursor,
    control: &BackupControl,
) -> Result<[u8; 32]> {
    control.checkpoint(1, 0)?;
    if sequence == 0 {
        return Ok([0; 32]);
    }
    if sequence == cursor.floor {
        return read
            .get(FLOORS, &origin.to_be_bytes())?
            .ok_or(Error::Corrupt("missing origin chain"))?
            .try_into()
            .map_err(|_| Error::Corrupt("origin chain length"));
    }
    if sequence < cursor.floor || sequence > cursor.applied {
        return Err(Error::InvalidInput("origin chain outside retained cursor"));
    }
    let bytes = read
        .get(EVENTS, &key(origin, sequence))?
        .ok_or(Error::Corrupt("missing retained origin chain"))?;
    if bytes.len() < EVENT_OVERHEAD {
        return Err(Error::Corrupt("journal record length"));
    }
    let event_end = bytes.len() - 32;
    let event = &bytes[..event_end];
    if event.len() < PAYLOAD_OVERHEAD || event[0] != 1 {
        return Err(Error::Corrupt("journal event header"));
    }
    let length = u32::from_be_bytes(event[25..29].try_into().unwrap()) as usize;
    if length.checked_add(PAYLOAD_OVERHEAD) != Some(event.len())
        || digest(&event[..event.len() - 32], control)?.as_slice() != &event[event.len() - 32..]
    {
        return Err(Error::Corrupt("journal event length or digest"));
    }
    if number(&event[1..9])? != origin || number(&event[9..17])? != sequence {
        return Err(Error::Corrupt("origin chain identity"));
    }
    Ok(bytes[event_end..].try_into().unwrap())
}

fn heads(
    read: &ReadTxn,
    cursors: &BTreeMap<u64, Cursor>,
    control: &BackupControl,
) -> Result<BTreeMap<u64, [u8; 32]>> {
    let mut result = BTreeMap::new();
    for (&origin, &cursor) in cursors {
        control.checkpoint(1, 0)?;
        if cursor.applied == 0 {
            return Err(Error::Corrupt("zero persisted origin cursor"));
        }
        let bytes = read
            .get(HEADS, &origin.to_be_bytes())?
            .ok_or(Error::Corrupt("missing journal head digest"))?;
        let hash: [u8; 32] = bytes
            .try_into()
            .map_err(|_| Error::Corrupt("journal head digest length"))?;
        if chain_at(read, origin, cursor.applied, cursor, control)? != hash {
            return Err(Error::Corrupt("journal head/cursor chain mismatch"));
        }
        if cursor.floor > 0 {
            chain_at(read, origin, cursor.floor, cursor, control)?;
        }
        result.insert(origin, hash);
    }
    Ok(result)
}

fn read_manifest(target: &Path, control: &BackupControl) -> Result<SnapshotReceipt> {
    control.checkpoint(1, 0)?;
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
    let mut input = File::open(path)?;
    let mut bytes = Vec::new();
    let mut block = [0; BLOCK];
    let read_limit = (metadata.len() as usize + 1).min(MAX_MANIFEST + 1);
    loop {
        let remaining = read_limit.saturating_sub(bytes.len());
        if remaining == 0 {
            return Err(Error::Corrupt("snapshot manifest length"));
        }
        let count = remaining.min(block.len());
        control.checkpoint(1, count as u64)?;
        let count = input.read(&mut block[..count])?;
        if count == 0 {
            break;
        }
        bytes.extend_from_slice(&block[..count]);
    }
    if bytes.len() < 32 || bytes.len() > MAX_MANIFEST {
        return Err(Error::Corrupt("snapshot manifest length"));
    }
    let id = digest(&bytes[..bytes.len() - 32], control)?;
    // The decoder is bounded by the one-MiB manifest envelope and4096 origins.
    control.checkpoint(1, bytes.len() as u64)?;
    super::decode_manifest(&bytes, id)
}

fn write_manifest(
    target: &Path,
    receipt: &mut SnapshotReceipt,
    control: &BackupControl,
) -> Result<()> {
    control.checkpoint(1, 0)?;
    let mut bytes = body(receipt)?;
    if bytes.len().checked_add(32).is_none_or(|n| n > MAX_MANIFEST) {
        return Err(Error::LimitExceeded);
    }
    receipt.id = digest(&bytes, control)?;
    bytes.extend_from_slice(&receipt.id);
    let pending = target.join(PENDING_MANIFEST);
    control.checkpoint(1, 0)?;
    let mut output = OpenOptions::new()
        .write(true)
        .create_new(true)
        .open(&pending)?;
    for chunk in bytes.chunks(BLOCK) {
        control.checkpoint(1, chunk.len() as u64)?;
        output.write_all(chunk)?;
    }
    control.checkpoint(1, 0)?;
    output.sync_all()?;
    // This is the outer bundle's publication boundary. No cancellation checks
    // follow a successful link: a late request cannot report an aborted bundle
    // after its complete, synchronized readiness manifest became visible.
    control.checkpoint(1, 0)?;
    fs::hard_link(&pending, target.join(MANIFEST))?;
    fs::remove_file(&pending).map_err(|_| Error::OutcomeUnknown)?;
    File::open(target)
        .and_then(|directory| directory.sync_all())
        .map_err(|_| Error::OutcomeUnknown)?;
    if let Some(parent) = target.parent() {
        File::open(if parent.as_os_str().is_empty() {
            Path::new(".")
        } else {
            parent
        })
        .and_then(|directory| directory.sync_all())
        .map_err(|_| Error::OutcomeUnknown)?;
    }
    Ok(())
}

/// Export one exact journal/native snapshot with cooperative cancellation and
/// aggregate work limits, including journal metadata and native file copying.
///
/// A stopped operation can leave its newly created target and pending files.
/// It never publishes `SNAPSHOT` or returns a receipt before all native and
/// journal verification succeeds. If cancellation arrives after the native
/// database commits but before the outer manifest, `database/CURRENT` can exist
/// while the journal bundle remains unready and is rejected by verification.
/// Cancelled operations do not remove caller-owned or preexisting paths.
pub fn export_snapshot_with_control(
    environment: &Environment,
    read: &ReadTxn,
    target: impl AsRef<Path>,
    control: &BackupControl,
) -> Result<SnapshotReceipt> {
    let target = target.as_ref();
    control.scope(|| {
        control.checkpoint(1, 0)?;
        let realm = store::realm(read)?;
        let cursors = cursors(read, control)?;
        let heads = heads(read, &cursors, control)?;
        let versions = store::versions(read)?;
        let baseline = baseline::provenance(read)?;
        control.checkpoint(1, 0)?;
        fs::create_dir(target)?;
        let native = backup::export_snapshot_with_control(
            environment,
            read,
            target.join("database"),
            control,
        )?;
        let mut receipt = SnapshotReceipt {
            realm,
            cursors,
            heads,
            versions,
            baseline,
            native,
            id: [0; 32],
        };
        write_manifest(target, &mut receipt, control)?;
        Ok(receipt)
    })
}

/// Verify a journal recovery bundle with cancellation and work limits spanning
/// manifest I/O, cold native opening/admission, index validation and origin heads.
pub fn verify_snapshot_with_control(
    target: impl AsRef<Path>,
    control: &BackupControl,
) -> Result<SnapshotReceipt> {
    let target = target.as_ref();
    control.scope(|| {
        let receipt = read_manifest(target, control)?;
        let database = target.join("database");
        control.checkpoint(1, 0)?;
        if ulmdb_storage::range::is_selected(&database)? {
            return Err(Error::InvalidInput(
                "controlled snapshot verification requires ordinary mapped files",
            ));
        }
        let environment = Environment::open(database)?;
        let read = environment.begin_read();
        let cursors = cursors(&read, control)?;
        if backup::verify_with_control(&environment, control)? != receipt.native
            || store::realm(&read)? != receipt.realm
            || store::versions(&read)? != receipt.versions
            || baseline::provenance(&read)? != receipt.baseline
            || cursors != receipt.cursors
            || heads(&read, &cursors, control)? != receipt.heads
        {
            return Err(Error::Corrupt(
                "snapshot metadata does not match native commit",
            ));
        }
        Ok(receipt)
    })
}

/// Start a resumable export of an already selected snapshot. The owned read and
/// environment remain alive across budget grants; the application's writer can
/// continue using another clone of the environment. The worker does not replay
/// earlier verification/copy work when `advance` grants the next slice.
pub fn export_snapshot_job(
    environment: Arc<Environment>,
    read: ReadTxn,
    target: impl AsRef<Path>,
    control: BackupControl,
) -> Result<BackupJob<SnapshotReceipt>> {
    let target = target.as_ref().to_path_buf();
    BackupJob::spawn(control, move |control| {
        export_snapshot_with_control(&environment, &read, target, &control)
    })
}

/// Start resumable verification including cold native opening/admission. No
/// environment is opened on the caller thread before its first budget grant.
pub fn verify_snapshot_job(
    target: impl AsRef<Path>,
    control: BackupControl,
) -> Result<BackupJob<SnapshotReceipt>> {
    let target = target.as_ref().to_path_buf();
    BackupJob::spawn(control, move |control| {
        verify_snapshot_with_control(target, &control)
    })
}
