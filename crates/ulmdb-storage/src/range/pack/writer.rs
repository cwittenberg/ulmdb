//! Experimental two-fence append publication. Never mutate a published frame.
use crate::WriterFence;
use crate::range::pack::{
    BLOCK, DATA_START, Error, HEAD_OFFSETS, Receipt, Result,
    codec::{self, Block, Frame, Global},
    io::{read_exact_at, write_all_at},
    point,
};
use sha2::{Digest, Sha256};
use std::{
    fs::{File, OpenOptions},
    io::Read,
    path::Path,
};
pub(crate) struct Writer {
    pub(crate) writer: WriterFence,
    pub(crate) reader: File,
    pub(crate) global: Global,
    pub(crate) selected: Receipt,
    pub(crate) slot: usize,
    pub(crate) poisoned: bool,
    pub(crate) preparing: bool,
}
struct FrameHooks {
    before_header: &'static str,
    half_header: &'static str,
    after_header: &'static str,
    half_payload: &'static str,
    after_payload: &'static str,
    before_footer: &'static str,
    half_footer: &'static str,
    after_footer: &'static str,
}
const COMMIT: FrameHooks = FrameHooks {
    before_header: "packed.commit.before_header",
    half_header: "packed.commit.half_header",
    after_header: "packed.commit.after_header",
    half_payload: "packed.commit.half_payload",
    after_payload: "packed.commit.after_payload",
    before_footer: "packed.commit.before_footer",
    half_footer: "packed.commit.half_footer",
    after_footer: "packed.commit.after_footer",
};
const GENESIS: FrameHooks = FrameHooks {
    before_header: "packed.create.genesis.before_header",
    half_header: "packed.create.genesis.half_header",
    after_header: "packed.create.genesis.after_header",
    half_payload: "packed.create.genesis.half_payload",
    after_payload: "packed.create.genesis.after_payload",
    before_footer: "packed.create.genesis.before_footer",
    half_footer: "packed.create.genesis.half_footer",
    after_footer: "packed.create.genesis.after_footer",
};
impl Writer {
    pub(crate) fn create(path: &Path) -> Result<Self> {
        Self::create_global(path, Global::initial(random_id()?))
    }
    pub(crate) fn create_global(path: &Path, global: Global) -> Result<Self> {
        let mut options = OpenOptions::new();
        options.create_new(true).read(true).write(true);
        #[cfg(unix)]
        {
            use std::os::unix::fs::OpenOptionsExt;
            options.mode(0o600);
        }
        point("packed.create.before_open")?;
        let writer = options.open(path)?;
        Self::initialize_created(path, global, writer)
    }
    // Rotation supplies a duplicate of its retained create_new descriptor.
    // Ordinary creation keeps its original residue/recovery contract.
    pub(crate) fn initialize_created(path: &Path, global: Global, writer: File) -> Result<Self> {
        // create_new plus an explicit owner mode also survives restrictive umask.
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            writer.set_permissions(std::fs::Permissions::from_mode(0o600))?;
        }
        let writer = lock_writer(writer)?;
        let reader = independent_reader(path, &writer)?;

        let encoded = codec::encode_global(&global);
        point("packed.create.before_global")?;
        write_with_cut(&writer, &encoded, 0, "packed.create.half_global")?;
        point("packed.create.after_global")?;
        let (genesis, header) = codec::encode_frame_header(
            global.file_id,
            DATA_START,
            0,
            0,
            None,
            Sha256::digest([]).into(),
        )?;
        write_frame(&writer, global.file_id, &genesis, &header, &[], &GENESIS)?;
        let head = codec::encode_head(global.file_id, genesis.receipt);
        point("packed.create.before_head0")?;
        write_with_cut(&writer, &head, HEAD_OFFSETS[0], "packed.create.half_head0")?;
        point("packed.create.after_head0")?;
        point("packed.create.before_head1")?;
        write_with_cut(&writer, &head, HEAD_OFFSETS[1], "packed.create.half_head1")?;
        point("packed.create.after_head1")?;
        point("packed.create.before_file_sync")?;
        ulmdb_platform::sync_file(&writer)?;
        point("packed.create.after_file_sync")?;
        point("packed.create.before_directory_sync")?;
        ulmdb_platform::sync_path(
            path.parent()
                .filter(|p| !p.as_os_str().is_empty())
                .unwrap_or(Path::new(".")),
        )?;
        point("packed.create.after_directory_sync")?;
        point("packed.create.before_ack")?;
        Ok(Self {
            writer,
            reader,
            global,
            selected: genesis.receipt,
            slot: 0,
            poisoned: false,
            preparing: false,
        })
    }
    pub(crate) fn open_checked<T, F: FnOnce(&File, [u8; 16], Receipt) -> Result<T>>(
        path: &Path,
        validate: F,
    ) -> Result<(Self, T)> {
        let writer = OpenOptions::new().read(true).write(true).open(path)?;
        let writer = lock_writer(writer)?;
        let reader = independent_reader(path, &writer)?;
        let mut header = [0; BLOCK];
        read_exact_at(&*writer, &mut header, 0)?;
        let global = codec::decode_global(&header)?;
        // Recovery owns strict head/frame/parent selection, tail trimming and
        // durable invalid-slot repair. It must complete them before readiness.
        let mut checked = None;
        let (selected, slot) =
            crate::range::pack::recovery::recover_checked(&writer, global.file_id, |receipt| {
                checked = Some(validate(&reader, global.file_id, receipt)?);
                Ok(())
            })?;
        if slot > 1 || writer.metadata()?.len() != selected.frame_end()? {
            return Err(Error::Corrupt("packed recovered extent/slot"));
        }
        // An accepted complete creation residue may never have synced its
        // directory entry; a visible head may follow a failed head sync. Fence
        // both before returning a ready writer or a newly adopted receipt.
        point("packed.open.before_file_sync")?;
        ulmdb_platform::sync_file(&writer)?;
        point("packed.open.after_file_sync")?;
        point("packed.open.before_directory_sync")?;
        let resolved = std::fs::canonicalize(path)?;
        ulmdb_platform::sync_path(
            resolved
                .parent()
                .ok_or(Error::InvalidInput("packed file has no parent"))?,
        )?;
        point("packed.open.after_directory_sync")?;
        point("packed.open.before_ready")?;
        Ok((
            Self {
                writer,
                reader,
                global,
                selected,
                slot,
                poisoned: false,
                preparing: false,
            },
            checked.ok_or(Error::Corrupt("missing checked range state"))?,
        ))
    }
    fn check_selected_heads(&self) -> Result<()> {
        if self.slot > 1 {
            return Err(Error::Corrupt("range active head slot"));
        }
        let mut blocks = [[0; BLOCK]; 2];
        for (block, offset) in blocks.iter_mut().zip(HEAD_OFFSETS) {
            read_exact_at(&*self.writer, block, offset)?;
        }
        let selected = codec::decode_head(&blocks[self.slot], self.global.file_id)?;
        let older = codec::decode_head(&blocks[1 - self.slot], self.global.file_id)?;
        if selected != self.selected {
            return Err(Error::Corrupt("range selected head changed"));
        }
        let frame = super::recovery::verify_envelope(&self.writer, self.global.file_id, selected)?;
        if selected.ordinal == 0 {
            if older != selected || frame.parent.is_some() {
                return Err(Error::Corrupt("range genesis heads disagree"));
            }
        } else if older.ordinal.checked_add(1) != Some(selected.ordinal)
            || frame.parent != Some(older)
        {
            return Err(Error::Corrupt("range head predecessor changed"));
        }
        Ok(())
    }
    pub(crate) fn prepare(&mut self, payload: &[u8]) -> Result<Frame> {
        if self.poisoned {
            return Err(Error::OutcomeUnknown);
        }
        if self.preparing {
            return Err(Error::Busy);
        }
        // Active writers never repair or normalize corrupt heads. The selected
        // root stays authoritative, and reopening owns validated recovery.
        if let Err(error) = self.check_selected_heads() {
            self.poisoned = true;
            return Err(error);
        }
        let offset = self.selected.frame_end()?;
        if self.writer.metadata()?.len() != offset {
            return Err(Error::Corrupt("range append has unselected tail"));
        }
        let ordinal = self
            .selected
            .ordinal
            .checked_add(1)
            .ok_or(Error::LimitExceeded)?;
        let length = crate::range::pack::frame_length(payload.len() as u64)?;
        offset
            .checked_add(length)
            .filter(|n| *n <= crate::range::pack::MAX_FILE_BYTES)
            .ok_or(Error::LimitExceeded)?;
        let (frame, header) = codec::encode_frame_header(
            self.global.file_id,
            offset,
            ordinal,
            payload.len() as u64,
            Some(self.selected),
            Sha256::digest(payload).into(),
        )?;
        let attempt = (|| {
            write_frame(
                &self.writer,
                self.global.file_id,
                &frame,
                &header,
                payload,
                &COMMIT,
            )?;
            point("packed.commit.before_frame_sync")?;
            ulmdb_platform::sync_file(&self.writer)?;
            point("packed.commit.after_frame_sync")?;
            Ok::<(), Error>(())
        })();
        if let Err(error) = attempt {
            // Heads are untouched and no private map exists yet. A complete
            // truncate+file fence proves nonpublication and restores readiness.
            if self
                .writer
                .set_len(offset)
                .and_then(|()| ulmdb_platform::sync_file(&self.writer))
                .is_err()
            {
                self.poisoned = true;
                return Err(Error::OutcomeUnknown);
            }
            return Err(error);
        }
        self.preparing = true;
        Ok(frame)
    }
    pub(crate) fn publish_prepared(&mut self, frame: &Frame) -> Result<Receipt> {
        if self.poisoned {
            return Err(Error::OutcomeUnknown);
        }
        if !self.preparing || frame.parent != Some(self.selected) || self.slot > 1 {
            return Err(Error::Corrupt("foreign prepared range frame"));
        }
        let inactive = 1 - self.slot;
        let result = (|| {
            let head = codec::encode_head(self.global.file_id, frame.receipt);
            point("packed.commit.before_head")?;
            write_with_cut(
                &self.writer,
                &head,
                HEAD_OFFSETS[inactive],
                "packed.commit.half_head",
            )?;
            point("packed.commit.after_head")?;
            point("packed.commit.before_head_sync")?;
            ulmdb_platform::sync_file(&self.writer)?;
            point("packed.commit.after_head_sync")?;
            point("packed.commit.before_ack")?;
            Ok::<(), Error>(())
        })();
        if result.is_err() {
            self.poisoned = true;
            return Err(Error::OutcomeUnknown);
        }
        self.selected = frame.receipt;
        self.slot = inactive;
        self.preparing = false;
        Ok(frame.receipt)
    }
    /// Caller has dropped EVERY private candidate map before this truncation.
    pub(crate) fn abort_prepared(&mut self) -> Result<()> {
        if self.poisoned {
            return Err(Error::OutcomeUnknown);
        }
        if self.preparing {
            let end = self.selected.frame_end()?;
            let result = self
                .writer
                .set_len(end)
                .and_then(|()| ulmdb_platform::sync_file(&self.writer));
            if result.is_err() {
                self.poisoned = true;
                return Err(Error::OutcomeUnknown);
            }
            self.preparing = false;
        }
        Ok(())
    }
}
fn lock_writer(file: File) -> Result<WriterFence> {
    WriterFence::try_lock(file).map_err(|error| match error {
        std::fs::TryLockError::WouldBlock => Error::Busy,
        std::fs::TryLockError::Error(error) => Error::Io(error),
    })
}
fn write_frame(
    file: &File,
    file_id: [u8; 16],
    frame: &Frame,
    header: &Block,
    payload: &[u8],
    hooks: &FrameHooks,
) -> Result<()> {
    point(hooks.before_header)?;
    write_with_cut(file, header, frame.receipt.offset, hooks.half_header)?;
    point(hooks.after_header)?;
    let payload_start = frame.receipt.offset + BLOCK as u64;
    write_with_cut(file, payload, payload_start, hooks.half_payload)?;
    point(hooks.after_payload)?;
    // This is a fresh appended extent (or creation). Positional footer writes
    // extend over sparse zero padding; no old published byte is overwritten.
    let footer = codec::encode_frame_footer(file_id, frame);
    let offset = frame.receipt.offset + frame.frame_length - BLOCK as u64;
    point(hooks.before_footer)?;
    write_with_cut(file, &footer, offset, hooks.half_footer)?;
    point(hooks.after_footer)?;
    Ok(())
}
fn random_id() -> Result<[u8; 16]> {
    let mut random = File::open("/dev/urandom")?;
    for _ in 0..16 {
        let mut id = [0; 16];
        random.read_exact(&mut id)?;
        if id != [0; 16] {
            return Ok(id);
        }
    }
    Err(Error::Io(std::io::Error::other(
        "random file identity unavailable",
    )))
}
fn independent_reader(path: &Path, writer: &File) -> Result<File> {
    let reader = File::open(path)?;
    #[cfg(unix)]
    {
        use std::os::unix::fs::MetadataExt;
        let a = writer.metadata()?;
        let b = reader.metadata()?;
        if !a.is_file() || !b.is_file() || a.dev() != b.dev() || a.ino() != b.ino() {
            return Err(Error::Corrupt("packed read handle identity mismatch"));
        }
    }
    #[cfg(not(unix))]
    {
        let _ = writer;
        return Err(Error::Io(std::io::ErrorKind::Unsupported.into()));
    }
    Ok(reader)
}

/// Production issues one positional write-all per buffer. Only the explicit
/// crash-testing feature splits a buffer to expose a deterministic torn cut.
fn write_with_cut(file: &File, bytes: &[u8], offset: u64, half_hook: &str) -> Result<()> {
    #[cfg(feature = "fault-injection")]
    {
        let half = bytes.len() / 2;
        write_all_at(file, &bytes[..half], offset)?;
        point(half_hook)?;
        write_all_at(file, &bytes[half..], offset + half as u64)?;
    }
    #[cfg(not(feature = "fault-injection"))]
    {
        let _ = half_hook;
        write_all_at(file, bytes, offset)?;
    }
    Ok(())
}
