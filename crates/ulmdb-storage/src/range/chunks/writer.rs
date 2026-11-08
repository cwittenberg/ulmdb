//! V7 append publication. Same-chunk commits use two ordered file fences.
//! No selected capability or mutable descriptor escapes this transport.
use super::{
    geometry::{self, AppendPlan, HEADER_BYTES},
    header::{self, ChunkBinding, ChunkHeader, Continuation, StreamId},
    protocol::{self, Frame, Global, Receipt},
    recovery::{self, Recovered},
};
use crate::range::pack::{
    io::{read_exact_at, write_all_at},
    point,
};
use crate::{Error, Result, WriterFence};
use sha2::{Digest, Sha256};
use std::{
    fs::{self, File, OpenOptions},
    path::{Path, PathBuf},
};

struct Active {
    file: File,
    binding: ChunkBinding,
    encoded: Box<[u8; HEADER_BYTES as usize]>,
}
struct Pending {
    frame: Frame,
    rollover: Option<Active>,
}
/// A writer-local preparation. Its projection is not a selected receipt.
pub(crate) struct Prepared {
    owner: u128,
    frame: Frame,
}
impl Prepared {
    pub(crate) fn receipt(&self) -> Receipt {
        self.frame.receipt
    }
    pub(crate) fn parent(&self) -> Option<Receipt> {
        self.frame.parent
    }
}
pub(crate) struct Writer {
    directory: PathBuf,
    global: Global,
    selected: Receipt,
    slot: usize,
    active: Active,
    pending: Option<Pending>,
    poisoned: bool,
    owner: u128,
    scratch: Box<[u8; HEADER_BYTES as usize]>,
    // Last field: all data-file write handles close before the control fence.
    control: WriterFence,
}
impl Writer {
    pub(crate) fn create(directory: &Path) -> Result<Self> {
        let owner = crate::new_identity()?;
        let owned_directory = directory.to_path_buf();
        let scratch = Box::new([0; HEADER_BYTES as usize]);
        if !directory.is_dir() {
            return Err(Error::InvalidInput("v7 directory missing"));
        }
        for name in [
            protocol::CONTROL_FILE,
            "RANGE.pack",
            "CURRENT",
            "PREVIOUS",
            "BACKEND",
        ] {
            if directory.join(name).try_exists()? {
                return Err(Error::InvalidInput("v7 create requires unpublished layout"));
            }
        }
        let control = lock(create_private(&directory.join(protocol::CONTROL_FILE))?)?;
        let global = Global {
            stream: StreamId::new(crate::new_identity()?.to_le_bytes())?,
        };
        let header = ChunkHeader::new(global.stream, 0, None)?;
        let path = directory.join(header.filename());
        let file = create_private(&path)?;
        let encoded_header = Box::new(header.encode());
        let binding = header.binding();
        point("chunks.create.before_chunk_header")?;
        write_all_at(&file, &*encoded_header, 0)?;
        point("chunks.create.after_chunk_header")?;
        let (frame, encoded) = protocol::encode_frame_header(
            global.stream,
            0,
            0,
            0,
            None,
            binding.header_hash,
            Sha256::digest([]).into(),
        )?;
        write_frame(&file, global.stream, frame, &encoded, &[], "create")?;
        point("chunks.create.before_data_sync")?;
        ulmdb_platform::sync_file(&file)?;
        point("chunks.create.after_data_sync")?;
        control.set_len(protocol::CONTROL_LEN)?;
        write_all_at(&*control, &protocol::encode_global(global), 0)?;
        let head = protocol::encode_head(global.stream, frame.receipt);
        write_part(
            &control,
            &head,
            protocol::HEAD_OFFSETS[0],
            "chunks.create.half_head0",
        )?;
        write_part(
            &control,
            &head,
            protocol::HEAD_OFFSETS[1],
            "chunks.create.half_head1",
        )?;
        point("chunks.create.before_control_sync")?;
        ulmdb_platform::sync_file(&control)?;
        point("chunks.create.after_control_sync")?;
        sync_directory(
            directory,
            "chunks.create.before_directory_sync",
            "chunks.create.after_directory_sync",
        )?;
        point("chunks.create.before_ack")?;
        Ok(Self {
            directory: owned_directory,
            global,
            selected: frame.receipt,
            slot: 0,
            active: Active {
                file,
                binding,
                encoded: encoded_header,
            },
            pending: None,
            poisoned: false,
            owner,
            scratch,
            control,
        })
    }
    pub(crate) fn open_checked<F: FnOnce(&Recovered) -> Result<()>>(
        directory: &Path,
        validate: F,
    ) -> Result<(Self, Recovered)> {
        let owner = crate::new_identity()?;
        let owned_directory = directory.to_path_buf();
        let scratch = Box::new([0; HEADER_BYTES as usize]);
        let control = lock(
            OpenOptions::new()
                .read(true)
                .write(true)
                .open(directory.join(protocol::CONTROL_FILE))?,
        )?;
        let recovered = recovery::recover_checked(&control, directory, validate)?;
        let binding = recovered.selected.binding();
        let path = directory.join(header::filename(binding.stream, binding.index));
        let file = OpenOptions::new().read(true).write(true).open(path)?;
        let mut raw = [0; HEADER_BYTES as usize];
        read_exact_at(&file, &mut raw, 0)?;
        let header = ChunkHeader::decode(&raw)?;
        if header.binding() != binding {
            return Err(Error::Corrupt("v7 ready chunk changed"));
        }
        let encoded = Box::new(raw);
        point("chunks.open.before_data_sync")?;
        ulmdb_platform::sync_file(&file)?;
        point("chunks.open.after_data_sync")?;
        ulmdb_platform::sync_file(&control)?;
        sync_directory(
            directory,
            "chunks.open.before_directory_sync",
            "chunks.open.after_directory_sync",
        )?;
        point("chunks.open.before_ready")?;
        let writer = Self {
            directory: owned_directory,
            global: recovered.global,
            selected: recovered.selected,
            slot: recovered.slot,
            active: Active {
                file,
                binding,
                encoded,
            },
            pending: None,
            poisoned: false,
            owner,
            scratch,
            control,
        };
        Ok((writer, recovered))
    }
    pub(crate) fn selected(&self) -> Receipt {
        self.selected
    }
    pub(crate) fn is_poisoned(&self) -> bool {
        self.poisoned
    }
    pub(crate) fn directory(&self) -> &Path {
        &self.directory
    }
    pub(crate) fn stream(&self) -> StreamId {
        self.global.stream
    }
    fn check_ready(&mut self) -> Result<()> {
        if self.poisoned {
            return Err(Error::OutcomeUnknown);
        }
        if self.pending.is_some() {
            return Err(Error::Busy);
        }
        if self.slot > 1 || self.control.metadata()?.len() != protocol::CONTROL_LEN {
            return Err(Error::Corrupt("v7 ready selector geometry"));
        }
        let mut a = [0; protocol::BLOCK];
        let mut b = [0; protocol::BLOCK];
        read_exact_at(&*self.control, &mut a, protocol::HEAD_OFFSETS[self.slot])?;
        read_exact_at(
            &*self.control,
            &mut b,
            protocol::HEAD_OFFSETS[1 - self.slot],
        )?;
        let selected = protocol::decode_head(&a, self.global.stream)?;
        let older = protocol::decode_head(&b, self.global.stream)?;
        if selected != self.selected
            || self.active.binding != selected.binding()
            || self.active.file.metadata()?.len() != physical_end(selected)
        {
            return Err(Error::Corrupt("v7 selected state changed"));
        }
        // Compare with the exact canonical image admitted once: no repeated
        // header encoding, SHA or allocation on the ordinary commit path.
        read_exact_at(&self.active.file, &mut *self.scratch, 0)?;
        if *self.scratch != *self.active.encoded {
            return Err(Error::Corrupt("v7 active chunk envelope changed"));
        }
        let frame = verify_envelope(&self.active.file, self.global.stream, selected)?;
        if (selected.ordinal() == 0 && (older != selected || frame.parent.is_some()))
            || (selected.ordinal() != 0 && frame.parent != Some(older))
        {
            return Err(Error::Corrupt("v7 selected predecessor changed"));
        }
        Ok(())
    }
    pub(crate) fn prepare(&mut self, payload: &[u8]) -> Result<Prepared> {
        if let Err(error) = self.check_ready() {
            if !matches!(error, Error::Busy | Error::OutcomeUnknown) {
                self.poisoned = true;
            }
            return Err(error);
        }
        let plan = AppendPlan::after(self.selected.descriptor(), payload.len() as u64)?;
        let payload_hash = Sha256::digest(payload).into();
        let (frame, rollover) = if plan.rollover {
            self.prepare_rollover(plan, payload, payload_hash)?
        } else {
            let (frame, encoded) = protocol::encode_frame_header(
                self.global.stream,
                plan.start,
                plan.ordinal,
                payload.len() as u64,
                Some(self.selected),
                self.active.binding.header_hash,
                payload_hash,
            )?;
            let result = (|| {
                write_frame(
                    &self.active.file,
                    self.global.stream,
                    frame,
                    &encoded,
                    payload,
                    "prepare",
                )?;
                point("chunks.prepare.before_data_sync")?;
                ulmdb_platform::sync_file(&self.active.file)?;
                point("chunks.prepare.after_data_sync")?;
                Ok::<(), Error>(())
            })();
            if let Err(error) = result {
                self.restore_active()?;
                return Err(error);
            }
            (frame, None)
        };
        self.pending = Some(Pending { frame, rollover });
        Ok(Prepared {
            owner: self.owner,
            frame,
        })
    }
    fn prepare_rollover(
        &mut self,
        plan: AppendPlan,
        payload: &[u8],
        hash: [u8; 32],
    ) -> Result<(Frame, Option<Active>)> {
        let next = ChunkHeader::new(
            self.global.stream,
            plan.chunk,
            Some(Continuation {
                prior_header_hash: self.active.binding.header_hash,
                parent: self.selected.descriptor(),
            }),
        )?;
        let final_path = self.directory.join(next.filename());
        if final_path.try_exists()? {
            return Err(Error::Corrupt("v7 unexpected next chunk"));
        }
        let purpose = format!("chunk-{}", plan.chunk);
        let (temporary, file) = crate::temporary::create(&self.directory, &purpose)?;
        owner_only(&file)?;
        let encoded_chunk = Box::new(next.encode());
        let next_binding = next.binding();
        let mut renamed = false;
        let result = (|| {
            point("chunks.prepare.before_chunk_header")?;
            write_all_at(&file, &*encoded_chunk, 0)?;
            point("chunks.prepare.after_chunk_header")?;
            let (frame, encoded) = protocol::encode_frame_header(
                self.global.stream,
                plan.start,
                plan.ordinal,
                payload.len() as u64,
                Some(self.selected),
                next_binding.header_hash,
                hash,
            )?;
            write_frame(
                &file,
                self.global.stream,
                frame,
                &encoded,
                payload,
                "prepare",
            )?;
            point("chunks.prepare.before_data_sync")?;
            ulmdb_platform::sync_file(&file)?;
            point("chunks.prepare.after_data_sync")?;
            point("chunks.prepare.before_chunk_rename")?;
            ulmdb_platform::rename_replace(&temporary, &final_path)
                .map_err(ulmdb_platform::rename_original_error)?;
            renamed = true;
            point("chunks.prepare.after_chunk_rename")?;
            sync_directory(
                &self.directory,
                "chunks.prepare.before_directory_sync",
                "chunks.prepare.after_directory_sync",
            )?;
            let writable = OpenOptions::new()
                .read(true)
                .write(true)
                .open(&final_path)?;
            same_inode(&file, &writable)?;
            if writable.metadata()?.len() != physical_end(frame.receipt) {
                return Err(Error::Corrupt("v7 prepared rollover EOF"));
            }
            Ok((
                frame,
                Some(Active {
                    file: writable,
                    binding: next_binding,
                    encoded: encoded_chunk,
                }),
            ))
        })();
        if result.is_err() {
            let path = if renamed { &final_path } else { &temporary };
            let cleanup =
                fs::remove_file(path).and_then(|()| ulmdb_platform::sync_path(&self.directory));
            if cleanup.is_err() {
                self.poisoned = true;
                return Err(Error::OutcomeUnknown);
            }
        }
        result
    }
    fn check_token(&self, token: &Prepared) -> Result<()> {
        if self.poisoned {
            return Err(Error::OutcomeUnknown);
        }
        if token.owner != self.owner
            || self.pending.as_ref().map(|p| p.frame) != Some(token.frame)
            || token.frame.parent != Some(self.selected)
        {
            return Err(Error::Corrupt("foreign v7 prepared frame"));
        }
        Ok(())
    }
    pub(crate) fn publish(&mut self, token: Prepared) -> Result<Receipt> {
        self.check_token(&token)?;
        let inactive = 1 - self.slot;
        let head = protocol::encode_head(self.global.stream, token.frame.receipt);
        let result = (|| {
            point("chunks.publish.before_head")?;
            write_part(
                &self.control,
                &head,
                protocol::HEAD_OFFSETS[inactive],
                "chunks.publish.half_head",
            )?;
            point("chunks.publish.after_head")?;
            point("chunks.publish.before_control_sync")?;
            ulmdb_platform::sync_file(&self.control)?;
            point("chunks.publish.after_control_sync")?;
            point("chunks.publish.before_ack")?;
            Ok::<(), Error>(())
        })();
        if result.is_err() {
            self.poisoned = true;
            return Err(Error::OutcomeUnknown);
        }
        let pending = self
            .pending
            .take()
            .ok_or(Error::Corrupt("missing v7 preparation"))?;
        if let Some(active) = pending.rollover {
            self.active = active;
        }
        self.selected = token.frame.receipt;
        self.slot = inactive;
        Ok(self.selected)
    }
    pub(crate) fn abort(&mut self, token: Prepared) -> Result<()> {
        self.check_token(&token)?;
        let pending = self
            .pending
            .take()
            .ok_or(Error::Corrupt("missing v7 preparation"))?;
        if let Some(active) = pending.rollover {
            drop(active);
            let path = self.directory.join(header::filename(
                token.frame.receipt.binding().stream,
                token.frame.receipt.binding().index,
            ));
            let result = (|| {
                point("chunks.abort.before_remove")?;
                fs::remove_file(path)?;
                sync_directory(
                    &self.directory,
                    "chunks.abort.before_directory_sync",
                    "chunks.abort.after_directory_sync",
                )
            })();
            if result.is_err() {
                self.poisoned = true;
                return Err(Error::OutcomeUnknown);
            }
            Ok(())
        } else {
            self.restore_active()
        }
    }
    fn restore_active(&mut self) -> Result<()> {
        let result = (|| {
            point("chunks.abort.before_truncate")?;
            self.active.file.set_len(physical_end(self.selected))?;
            point("chunks.abort.after_truncate")?;
            ulmdb_platform::sync_file(&self.active.file)?;
            point("chunks.abort.after_sync")?;
            Ok::<(), Error>(())
        })();
        if result.is_err() {
            self.poisoned = true;
            Err(Error::OutcomeUnknown)
        } else {
            Ok(())
        }
    }
}
fn create_private(path: &Path) -> Result<File> {
    let mut options = OpenOptions::new();
    options.read(true).write(true).create_new(true);
    #[cfg(unix)]
    {
        use std::os::unix::fs::OpenOptionsExt;
        options.mode(0o600);
    }
    let file = options.open(path)?;
    owner_only(&file)?;
    Ok(file)
}
fn owner_only(file: &File) -> Result<()> {
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        file.set_permissions(std::fs::Permissions::from_mode(0o600))?;
    }
    #[cfg(not(unix))]
    let _ = file;
    Ok(())
}
fn physical_end(receipt: Receipt) -> u64 {
    HEADER_BYTES + receipt.end() - receipt.binding().index * geometry::DATA_BYTES
}
fn lock(file: File) -> Result<WriterFence> {
    WriterFence::try_lock(file).map_err(|e| match e {
        std::fs::TryLockError::WouldBlock => Error::Busy,
        std::fs::TryLockError::Error(e) => e.into(),
    })
}
fn same_inode(a: &File, b: &File) -> Result<()> {
    #[cfg(unix)]
    {
        use std::os::unix::fs::MetadataExt;
        let a = a.metadata()?;
        let b = b.metadata()?;
        if !a.is_file() || !b.is_file() || a.dev() != b.dev() || a.ino() != b.ino() {
            return Err(Error::Corrupt("v7 file handle identity mismatch"));
        }
        Ok(())
    }
    #[cfg(not(unix))]
    {
        let _ = (a, b);
        Err(Error::Io(std::io::ErrorKind::Unsupported.into()))
    }
}
fn sync_directory(dir: &Path, before: &str, after: &str) -> Result<()> {
    point(before)?;
    ulmdb_platform::sync_path(dir)?;
    point(after)
}
fn verify_envelope(file: &File, stream: StreamId, receipt: Receipt) -> Result<Frame> {
    let mut bytes = [0; protocol::BLOCK];
    read_exact_at(file, &mut bytes, geometry::locate(receipt.start()).1)?;
    let frame = protocol::decode_frame_header_bound(&bytes, receipt.binding())?;
    if frame.receipt != receipt || stream != receipt.binding().stream {
        return Err(Error::Corrupt("v7 selected envelope changed"));
    }
    let offset = physical_end(receipt) - protocol::BLOCK as u64;
    read_exact_at(file, &mut bytes, offset)?;
    protocol::decode_frame_footer(&bytes, stream, &frame)?;
    Ok(frame)
}
fn write_frame(
    file: &File,
    stream: StreamId,
    frame: Frame,
    header: &protocol::Block,
    payload: &[u8],
    phase: &str,
) -> Result<()> {
    let start = geometry::locate(frame.receipt.start()).1;
    let hooks = if phase == "create" {
        [
            "chunks.create.before_header",
            "chunks.create.half_header",
            "chunks.create.after_header",
            "chunks.create.half_payload",
            "chunks.create.after_payload",
            "chunks.create.before_footer",
            "chunks.create.half_footer",
            "chunks.create.after_footer",
        ]
    } else {
        [
            "chunks.prepare.before_header",
            "chunks.prepare.half_header",
            "chunks.prepare.after_header",
            "chunks.prepare.half_payload",
            "chunks.prepare.after_payload",
            "chunks.prepare.before_footer",
            "chunks.prepare.half_footer",
            "chunks.prepare.after_footer",
        ]
    };
    point(hooks[0])?;
    write_part(file, header, start, hooks[1])?;
    point(hooks[2])?;
    write_part(file, payload, start + protocol::BLOCK as u64, hooks[3])?;
    point(hooks[4])?;
    point(hooks[5])?;
    write_part(
        file,
        &protocol::encode_frame_footer(stream, &frame),
        physical_end(frame.receipt) - protocol::BLOCK as u64,
        hooks[6],
    )?;
    point(hooks[7])
}
fn write_part(file: &File, bytes: &[u8], offset: u64, half: &str) -> Result<()> {
    #[cfg(feature = "fault-injection")]
    {
        let count = bytes.len() / 2;
        write_all_at(file, &bytes[..count], offset)?;
        point(half)?;
        write_all_at(file, &bytes[count..], offset + count as u64)?;
    }
    #[cfg(not(feature = "fault-injection"))]
    {
        let _ = half;
        write_all_at(file, bytes, offset)?;
    }
    Ok(())
}
