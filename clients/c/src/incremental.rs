//! LMDB 1.0 incremental entry points over native immutable-asset deltas.
use super::*;
use std::fs::{File, OpenOptions};

fn duplicate(fd: FileHandle) -> FfiResult<File> {
    ulmdb_platform::duplicate_file_handle(fd).map_err(|e| code(e.into()))
}
fn capture(shared: &Shared) -> FfiResult<ulmdb_storage::coordination::DirectoryCapture> {
    match &shared.coordination {
        Some(c) => c.capture_directory().map_err(code),
        None => ulmdb_storage::coordination::Coordination::capture_directory_with_transform(
            &shared.directory,
            Default::default(),
            shared.transform.clone(),
        )
        .map_err(code),
    }
}
fn dump(env: &MDB_env, output: &mut File, since: usize) -> FfiResult<()> {
    let shared = env.shared.as_ref().ok_or(libc::EINVAL)?;
    shared.check_process()?;
    if since == 0 {
        return Err(libc::EINVAL);
    }
    if shared.poisoned.load(std::sync::atomic::Ordering::Acquire) {
        return Err(-30795);
    }
    if env.flags() & visibility::PREVIOUS != 0 {
        return Err(libc::ENOTSUP);
    }
    let _operation = rollback::OpeningTransaction::new(shared.clone())?;
    if let Some(external) = &shared.external {
        let read = external
            .begin_read(
                &shared.directory,
                shared.read_ahead(),
                shared.transform.clone(),
            )
            .map_err(code)?;
        read.dump_incremental_snapshot(&shared.directory, since as u64, output, Default::default())
            .map_err(code)?;
        return Ok(());
    }
    // Keep reclamation excluded across metadata choice, graph admission and the
    // complete stream, while allowing ordinary source writers to continue.
    let _outer_capture = capture(shared)?;
    let namespace = capture(shared)?;
    let lease = shared
        .visibility
        .as_ref()
        .map(|v| v.lease(false))
        .transpose()?;
    let frame = ulmdb::OwnedFileCapture::current(&shared.directory, namespace).map_err(code)?;
    if let (Some(v), Some(lease)) = (&shared.visibility, lease.as_ref()) {
        let record = v.read(lease)?;
        let selected =
            visibility::Meta::new(frame.selected_manifest(), *frame.selected_checkpoint())?;
        if record.matches(selected)? && record.kind == visibility::Kind::Offset {
            return Err(libc::ENOTSUP);
        }
    }
    drop(lease);
    let (read, _) = frame
        .admit_with_transform(
            ulmdb::ResidencyPolicy::Demand,
            shared.read_ahead(),
            shared.transform.clone(),
        )
        .map_err(code)?;
    read.dump_incremental_snapshot(&shared.directory, since as u64, output, Default::default())
        .map_err(code)?;
    Ok(())
}
#[unsafe(no_mangle)]
pub unsafe extern "C" fn mdb_env_incr_dumpfd(
    env: *mut MDB_env,
    fd: FileHandle,
    txnid: usize,
) -> c_int {
    boundary(|| {
        let env = unsafe { get_ref(env)? };
        env.shared.as_ref().ok_or(libc::EINVAL)?.check_process()?;
        let mut file = duplicate(fd)?;
        dump(env, &mut file, txnid)
    })
}
#[unsafe(no_mangle)]
pub unsafe extern "C" fn mdb_env_incr_dump(
    env: *mut MDB_env,
    path: *const c_char,
    txnid: usize,
) -> c_int {
    boundary(|| {
        let env = unsafe { get_ref(env)? };
        let path = unsafe { filesystem_path(path)? };
        let path = path.as_ref();
        let shared = env.shared.as_ref().ok_or(libc::EINVAL)?;
        shared.check_process()?;
        if let Some(external) = &shared.external {
            if shared.poisoned.load(std::sync::atomic::Ordering::Acquire) {
                return Err(-30795);
            }
            // Validate the original bound namespace before creating an output.
            let _ = external
                .begin_read(
                    &shared.directory,
                    shared.read_ahead(),
                    shared.transform.clone(),
                )
                .map_err(code)?;
        }
        // Pinned modern path-dump marks its output as a plain file before
        // opening it, even if that open fails. Source I/O keeps using the
        // already bound shared.directory; the FD entry point does not do this.
        if shared.catalog_terminator {
            shared
                .flags
                .fetch_or(0x4000, std::sync::atomic::Ordering::AcqRel);
        }
        let mut options = OpenOptions::new();
        options.create_new(true).write(true);
        #[cfg(unix)]
        {
            use std::os::unix::fs::OpenOptionsExt;
            options.mode(0o600);
        }
        let mut output = options.open(path).map_err(|e| code(e.into()))?;
        dump(env, &mut output, txnid)?;
        ulmdb_platform::sync_file(&output).map_err(|e| code(e.into()))
    })
}
pub(super) fn logical_size(read: &ReadTxn, shared: &Shared) -> FfiResult<usize> {
    let mut size = 8192usize;
    for name in read.database_names().map_err(code)? {
        if name != MAIN && !names::is_alias(&name) && name.first().is_some_and(|x| *x == 0) {
            continue;
        }
        if !matches!(
            read.database_kind(&name).map_err(code)?,
            DatabaseKind::Kv | DatabaseKind::Duplicates { .. }
        ) {
            continue;
        }
        let name_bytes = names::external_name_bytes(read, &name, shared)?;
        size = size.checked_add(name_bytes + 48).ok_or(-30792)?;
        let logical = read.logical_database_size(&name).map_err(code)?;
        let bytes = usize::try_from(logical.tuple_bytes().map_err(code)?).map_err(|_| -30792)?;
        size = size.checked_add(bytes).ok_or(-30792)?;
    }
    Ok(size)
}
#[unsafe(no_mangle)]
pub unsafe extern "C" fn mdb_env_incr_loadfd(env: *mut MDB_env, fd: FileHandle) -> c_int {
    boundary(|| {
        let env = unsafe { get_ref(env)? };
        let shared = env.shared.as_ref().ok_or(libc::EINVAL)?.clone();
        shared.check_process()?;
        if env.flags() & RDONLY != 0 {
            return Err(shared.readonly_error());
        }
        // Encryption implies REMAP_CHUNKS, which deliberately clears WRITEMAP
        // on open. Native delta import streams immutable assets and needs no
        // writable plaintext mapping; both admitted storage modes can load it.
        if env.flags() & (0x80000 | 0x4000000) == 0 {
            return Err(libc::EINVAL);
        }
        if shared.poisoned.load(std::sync::atomic::Ordering::Acquire) {
            return Err(-30795);
        }
        if shared.previous.pending() {
            return Err(libc::EBUSY);
        }
        let lifecycle = shared.lifecycle.try_lock().map_err(|_| libc::EBUSY)?;
        if *lifecycle != 0 || shared.writer_thread.lock().map_err(|_| BAD_TXN)?.is_some() {
            return Err(libc::EBUSY);
        }
        let mut input = duplicate(fd)?;
        let _exclusive = if shared.external.is_some() {
            None
        } else {
            Some(process::RollbackLease::acquire(
                &shared.directory,
                shared._environment_lease.as_ref(),
            )?)
        };
        let _writer = if shared.external.is_some() {
            None
        } else {
            Some(process::WriterLease::acquire(&shared.directory)?)
        };
        let owner = rollback::WriterEnvironment::open(shared.clone())?;
        let admission_error = std::cell::Cell::new(0);
        let result =
            owner.load_incremental_checked(&mut input, |read| match logical_size(read, &shared) {
                Ok(size) if size <= env.mapsize => Ok(()),
                Ok(_) => {
                    admission_error.set(-30792);
                    Err(Error::LimitExceeded)
                }
                Err(error) => {
                    admission_error.set(error);
                    Err(Error::InvalidInput("incremental facade admission"))
                }
            });
        match result {
            Ok(report) if report.files == 0 && report.base_sequence == report.sequence => {
                return Ok(());
            }
            Ok(_) => {}
            Err(error) => {
                if matches!(error, Error::OutcomeUnknown) {
                    shared
                        .poisoned
                        .store(true, std::sync::atomic::Ordering::Release);
                }
                return Err(if admission_error.get() != 0 {
                    admission_error.get()
                } else {
                    code(error)
                });
            }
        }
        let lease = shared
            .visibility
            .as_ref()
            .map(|v| v.lease(true))
            .transpose()
            .map_err(|_| {
                shared
                    .poisoned
                    .store(true, std::sync::atomic::Ordering::Release);
                -30795
            })?;
        if previous::publish_record(
            &shared,
            &owner,
            &previous::WriteRole::canonical(),
            lease.as_ref(),
        )
        .is_err()
        {
            shared
                .poisoned
                .store(true, std::sync::atomic::Ordering::Release);
            return Err(-30795);
        }
        drop(lease);
        drop(lifecycle);
        Ok(())
    })
}
