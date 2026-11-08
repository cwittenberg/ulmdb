use super::*;
// Serialize initialization of a pre-created directory with the engine's own
// writer fence. The fence inode is never replaced when staged files move in.
fn initialize_empty_directory(
    path: &std::path::Path,
    page_size: ulmdb::PageSize,
    access: ulmdb_storage::access::FileAccess,
    transform: Transform,
    external: bool,
    cache: chunks::Options,
) -> FfiResult<()> {
    fn empty_except_lock(path: &std::path::Path) -> FfiResult<()> {
        for entry in std::fs::read_dir(path).map_err(|e| code(e.into()))? {
            if entry.map_err(|e| code(e.into()))?.file_name() != "LOCK" {
                return Err(-30793);
            }
        }
        Ok(())
    }
    empty_except_lock(path)?;
    access.validate_root(path).map_err(code)?;
    let mut lock_options = std::fs::OpenOptions::new();
    #[cfg(unix)]
    {
        use std::os::unix::fs::OpenOptionsExt;
        lock_options
            .mode(access.mode())
            .custom_flags(libc::O_NOFOLLOW | libc::O_CLOEXEC);
    }
    let file = lock_options
        .read(true)
        .write(true)
        .create(true)
        .truncate(false)
        .open(path.join("LOCK"))
        .map_err(|e| code(e.into()))?;
    let actual_access = ulmdb_storage::access::FileAccessContext::open(path).map_err(code)?;
    if access.group().is_some() && actual_access.policy().group() != access.group() {
        return Err(libc::EACCES);
    }
    actual_access.validate_file(&file).map_err(code)?;
    let fence = if external {
        None
    } else {
        Some(ulmdb_storage::WriterFence::try_lock(file).map_err(|_| libc::EBUSY)?)
    };
    // Another initializer can finish between the first scan and lock acquisition.
    if path.join("CURRENT").exists() {
        drop(fence);
        return if external {
            ReadTxn::open_externally_synchronized_with_transform_options(
                path,
                Default::default(),
                transform,
            )
        } else {
            ReadTxn::open_snapshot_with_cache_options(path, Default::default(), transform, cache)
        }
        .map(|_| ())
        .map_err(code);
    }
    empty_except_lock(path)?;
    let parent = path
        .parent()
        .filter(|p| !p.as_os_str().is_empty())
        .unwrap_or(std::path::Path::new("."));
    let staging = copy::Staging::absent(parent)?;
    let stage = staging.path();
    let create = if external {
        Environment::create_externally_synchronized
    } else {
        Environment::create_with_file_access
    };
    drop(
        create(
            stage,
            ulmdb::PublicationMode::Legacy,
            page_size,
            Default::default(),
            Default::default(),
            transform.clone(),
            access,
        )
        .map_err(code)?,
    );
    let moved = (|| -> std::io::Result<()> {
        for entry in std::fs::read_dir(stage)? {
            let entry = entry?;
            if entry.file_name() == "CURRENT" {
                continue;
            }
            if entry.file_name() == "LOCK" {
                std::fs::remove_file(entry.path())?;
            } else {
                ulmdb_platform::rename_replace(&entry.path(), &path.join(entry.file_name()))
                    .map_err(ulmdb_platform::rename_original_error)?;
            }
        }
        ulmdb_platform::sync_path(path)?;
        ulmdb_platform::rename_replace(&stage.join("CURRENT"), &path.join("CURRENT"))?;
        ulmdb_platform::sync_path(path)?;
        std::fs::remove_dir(stage)?;
        let parent = path
            .parent()
            .filter(|p| !p.as_os_str().is_empty())
            .unwrap_or(std::path::Path::new("."));
        ulmdb_platform::sync_path(parent)
    })();
    moved.map_err(|error| {
        if ulmdb_platform::rename_outcome_unknown(&error) {
            code(ulmdb::Error::OutcomeUnknown)
        } else {
            code(error.into())
        }
    })?;
    drop(fence);
    ReadTxn::open_snapshot_with_cache_options(path, Default::default(), transform, cache)
        .map(|_| ())
        .map_err(code)
}
fn initialize_absent_directory(
    path: &std::path::Path,
    page_size: ulmdb::PageSize,
    access: ulmdb_storage::access::FileAccess,
    transform: Transform,
    external: bool,
    cache: chunks::Options,
) -> FfiResult<()> {
    let parent = path
        .parent()
        .filter(|p| !p.as_os_str().is_empty())
        .unwrap_or(std::path::Path::new("."));
    let stage = copy::Staging::absent(parent)?;
    {
        let create = if external {
            Environment::create_externally_synchronized
        } else {
            Environment::create_with_file_access
        };
        let _engine = create(
            stage.path(),
            ulmdb::PublicationMode::Legacy,
            page_size,
            Default::default(),
            Default::default(),
            transform,
            access,
        )
        .map_err(code)?;
    }
    stage.publish(path)
}
#[unsafe(no_mangle)]
pub unsafe extern "C" fn mdb_env_create(out: *mut *mut MDB_env) -> c_int {
    boundary(|| {
        *unsafe { get_mut(out)? } = Box::into_raw(Box::new(MDB_env {
            shared: None,
            path: None,
            flags: Arc::new(std::sync::atomic::AtomicU32::new(0)),
            mapsize: 10_485_760,
            mapsize_explicit: false,
            page_size: Default::default(),
            shared_group: None,
            crypto: Default::default(),
            maxdbs: 0,
            maxreaders: 126,
            userctx: ptr::null_mut(),
            data_file: None,
            assert_callback: None,
        }));
        Ok(())
    })
}
#[unsafe(no_mangle)]
pub unsafe extern "C" fn mdb_env_open(
    env: *mut MDB_env,
    path: *const c_char,
    flags: u32,
    mode: FileMode,
) -> c_int {
    boundary(|| {
        let env = unsafe { get_mut(env)? };
        if env.shared.is_some() {
            return Err(libc::EINVAL);
        }
        // Admit exact mapping policy combinations before opening any namespace.
        if flags & !0x7ff4001 != 0 {
            return Err(libc::EINVAL);
        }
        if flags & 1 != 0 && !cfg!(any(target_os = "linux", target_os = "macos", windows)) {
            return Err(libc::ENOTSUP);
        }
        let mut flags = flags | env.flags();
        if flags & 1 != 0 && flags & 0x4000000 != 0 {
            return Err(libc::EINVAL);
        }
        if flags & 0x4000000 != 0 {
            flags &= !0x80000;
            if flags & 1 != 0 {
                return Err(libc::EINVAL);
            }
        }
        let logical_path = unsafe { filesystem_path(path)? };
        let logical_path = logical_path.as_ref();
        // Recover published capacity before immutable transform/cache budgets.
        // Metadata discovery does not replace graph or namespace admission.
        let initial_path = storage_path::location(logical_path, flags);
        if !env.mapsize_explicit
            && let Some(size) = map_size::read(&initial_path)?
        {
            env.mapsize = size;
        }
        let cache = chunks::options(flags, env.mapsize)?;
        let transform = env.crypto.context(env.mapsize)?;
        if flags & RDONLY != 0 {
            flags &= !0x80000; // LMDB ignores WRITEMAP for read-only environments.
        }
        let native_path = storage_path::native_path_with_cache_options(
            logical_path,
            flags,
            transform.clone(),
            cache.clone(),
        )?;
        let pathbuf = native_path.as_path();
        let access = if pathbuf.join("CURRENT").exists() {
            let actual = ulmdb_storage::access::FileAccessContext::open(pathbuf)
                .map_err(code)?
                .policy();
            if env.shared_group.is_some() && env.shared_group != actual.group() {
                return Err(libc::EACCES);
            }
            actual
        } else {
            match env.shared_group {
                Some(gid) => {
                    ulmdb_storage::access::FileAccess::shared_group(requested_file_mode(mode), gid)
                }
                None => {
                    #[cfg(unix)]
                    {
                        use std::os::unix::fs::MetadataExt;
                        if let Ok(meta) = std::fs::symlink_metadata(pathbuf) {
                            if meta.is_dir()
                                && meta.mode() & 0o2072 == 0o2070
                                && mode & 0o060 == 0o060
                            {
                                ulmdb_storage::access::FileAccess::shared_group(
                                    requested_file_mode(mode),
                                    meta.gid(),
                                )
                            } else {
                                ulmdb_storage::access::FileAccess::requested(requested_file_mode(
                                    mode,
                                ))
                            }
                        } else {
                            ulmdb_storage::access::FileAccess::requested(requested_file_mode(mode))
                        }
                    }
                    #[cfg(not(unix))]
                    {
                        ulmdb_storage::access::FileAccess::requested(requested_file_mode(mode))
                    }
                }
            }
            .map_err(code)?
        };
        if let Some(published) = map_size::read(pathbuf)?
            && !env.mapsize_explicit
        {
            env.mapsize = published;
            if let Some(context) = &transform {
                context.raise_decoded_byte_limit(crypto::decoded_limit(published));
            }
        }
        if !pathbuf.join("CURRENT").exists() {
            if flags & RDONLY != 0 {
                return Err(libc::ENOENT);
            } else if pathbuf.is_dir() {
                initialize_empty_directory(
                    pathbuf,
                    env.page_size,
                    access,
                    transform.clone(),
                    flags & 0x400000 != 0
                        && cfg!(any(target_os = "linux", target_os = "macos", windows)),
                    cache.clone(),
                )?;
            } else {
                initialize_absent_directory(
                    pathbuf,
                    env.page_size,
                    access,
                    transform.clone(),
                    flags & 0x400000 != 0
                        && cfg!(any(target_os = "linux", target_os = "macos", windows)),
                    cache.clone(),
                )?;
            }
        }
        let transform_uuid =
            ulmdb_storage::transform::check_configuration(pathbuf, transform.as_ref())
                .map_err(code)?;
        let marker = std::fs::read_to_string(pathbuf.join("LMDB-ABI")).ok();
        let requested = std::env::var("ULMDB_LMDB_ABI").ok();
        let profile = alias::Profile::select(marker.as_deref(), requested.as_deref());
        let catalog_terminator = profile.catalog_terminator();
        let fixed_map = fixed_map::open(pathbuf, env, flags, catalog_terminator)?;
        let previous = flags & visibility::PREVIOUS != 0;
        if previous
            && (!catalog_terminator
                || !cfg!(any(target_os = "linux", target_os = "macos", windows)))
        {
            return Err(if catalog_terminator {
                libc::ENOTSUP
            } else {
                libc::EINVAL
            });
        }
        // Admission precedes chosen-view capture: a blocked SH waiter must not
        // reuse canonical data admitted before it wakes into a retained A view.
        let environment_lease = if flags & 0x400000 == 0 {
            let (lease, limit) = process::EnvironmentLease::begin(
                pathbuf,
                env.maxreaders,
                previous && flags & RDONLY == 0,
            )?;
            env.maxreaders = limit;
            Some(lease)
        } else {
            None
        };
        let visibility =
            previous::open_visibility(pathbuf, environment_lease.as_ref(), transform.clone())?;
        // Writable previous ownership retains native LOCK across clean/abort
        // until first publication. No Shared back-reference lives in this Arc.
        let _opening_writer = if previous && flags & RDONLY == 0 && flags & 0x400000 == 0 {
            Some(process::WriterLease::acquire(pathbuf)?)
        } else {
            None
        };
        let pending = if previous && flags & RDONLY == 0 {
            Some(Arc::new(
                if flags & 0x400000 != 0 {
                    ulmdb::PreviousSession::open_externally_synchronized_with_cache_options(
                        pathbuf,
                        ulmdb::ResidencyPolicy::Demand,
                        read_ahead(flags),
                        transform.clone(),
                        cache.clone(),
                    )
                } else {
                    ulmdb::PreviousSession::open_with_cache_options(
                        pathbuf,
                        ulmdb::ResidencyPolicy::Demand,
                        read_ahead(flags),
                        transform.clone(),
                        cache.clone(),
                    )
                }
                .map_err(previous::error)?,
            ))
        } else {
            None
        };
        let (coordination, initial) = if previous {
            let (read, durable, view) = if flags & 0x400000 != 0 {
                previous::initial_previous_external(
                    pathbuf,
                    read_ahead(flags),
                    transform.clone(),
                    cache.clone(),
                )?
            } else {
                previous::initial_previous(
                    pathbuf,
                    visibility.as_ref(),
                    read_ahead(flags),
                    transform.clone(),
                    cache.clone(),
                )?
            };
            if let Some(owner) = &pending
                && owner.selected_checkpoint().map_err(code)? != durable.checkpoint
            {
                return Err(-30795);
            }
            let read = if let Some(visibility) = &visibility {
                previous::install(
                    pathbuf,
                    visibility,
                    read,
                    durable,
                    view,
                    read_ahead(flags),
                    transform.clone(),
                    cache.clone(),
                )?
            } else {
                read
            };
            let coordination = if flags & 0x400000 == 0 {
                Some(
                    ulmdb::CoordinatedEnvironment::from_admitted_file_read(
                        pathbuf,
                        &read,
                        ulmdb::ResidencyPolicy::Demand,
                    )
                    .map_err(code)?,
                )
            } else {
                None
            };
            (coordination, read)
        } else if flags & 0x400000 != 0
            || !cfg!(any(target_os = "linux", target_os = "macos", windows))
        {
            (
                None,
                client_snapshot(
                    pathbuf,
                    true,
                    false,
                    read_ahead(flags),
                    transform.clone(),
                    cache.clone(),
                )?,
            )
        } else {
            let (coordination, initial) =
                ulmdb::CoordinatedEnvironment::open_with_initial_read_and_cache_options(
                    pathbuf,
                    ulmdb::ResidencyPolicy::Demand,
                    read_ahead(flags),
                    transform.clone(),
                    cache.clone(),
                )
                .map_err(code)?;
            (Some(coordination), initial)
        };
        #[cfg(any(target_os = "linux", target_os = "macos", windows))]
        if let Some(fixed) = &fixed_map {
            let m = initial.file_manifest().map_err(code)?;
            fixed.check_for(pathbuf, m.uuid, m.epoch).map_err(code)?;
            ulmdb::FixedMap::validate_layout(pathbuf, &[m]).map_err(code)?;
        }
        #[cfg(any(target_os = "linux", target_os = "macos", windows))]
        let fixed_declaration = if fixed_map.is_none() && pathbuf.join("FIXEDMAP").exists() {
            let m = initial.file_manifest().map_err(code)?;
            let declaration =
                ulmdb::FixedMapDeclaration::open(pathbuf, m.uuid, m.epoch).map_err(code)?;
            if flags & 0x400000 != 0 && declaration.supports_reuse().map_err(code)? {
                return Err(libc::EINVAL);
            }
            Some(declaration)
        } else {
            None
        };
        env.page_size = initial.file_manifest().map_err(code)?.page_size;
        if transform_uuid.is_some_and(|uuid| initial.file_manifest().is_ok_and(|m| m.uuid != uuid))
        {
            return Err(-30776);
        }
        let external = initial.external_synchronization();
        let initial = initial.project_empty_kv(MAIN).map_err(code)?;
        let main = initial.open_database(MAIN).ok();
        if let Some(published) = map_size::read(pathbuf)?
            && !env.mapsize_explicit
        {
            env.mapsize = published;
        }
        #[cfg(any(target_os = "linux", target_os = "macos", windows))]
        if let Some(fixed) = &fixed_map {
            let capacity = fixed.info().map_err(code)?.capacity;
            if env.mapsize_explicit && env.mapsize != capacity {
                return Err(libc::ENOTSUP);
            }
            env.mapsize = capacity;
        }
        if !previous && let Some(lease) = &environment_lease {
            lease.downgrade()?;
        }
        storage_path::publish_nosubdir_marker(logical_path, flags, mode)?;
        env.data_file =
            Some(std::fs::File::open(pathbuf.join("CURRENT")).map_err(|e| code(e.into()))?);
        let shared = Arc::new(Shared {
            cache: cache.clone(),
            directory: std::fs::canonicalize(pathbuf).map_err(|e| code(e.into()))?,
            transform,
            flags: env.flags.clone(),
            options: Mutex::new(std::collections::BTreeMap::new()),
            alias_ids: Default::default(),
            // LMDB's default database capability is DBI 1; slot zero is
            // reserved for its internal free database and never exposed here.
            handles: Mutex::new(vec![
                None,
                main.map(|native| {
                    let exported = compat_features::Settings::for_kind(native.kind());
                    AliasHandle::new(AliasId::MAIN, MAIN, native, exported, false)
                }),
            ]),
            aliases: Mutex::new(Default::default()),
            writer_thread: Mutex::new(None),
            lifecycle: Mutex::new(0),
            maintenance: Mutex::new(maintenance::Owner::for_open(flags)),
            map_bounds: Mutex::new(Default::default()),
            database_id_floor: std::sync::atomic::AtomicU64::new(
                pending.as_ref().map_or(1, |p| p.database_id_floor()),
            ),
            rollback_invalid: Mutex::new(Default::default()),
            maxdbs: env.maxdbs,
            maxreaders: env.maxreaders,
            page_size: env.page_size,
            readers: std::sync::atomic::AtomicU32::new(0),
            reader_threads: Mutex::new(Vec::new()),
            poisoned: std::sync::atomic::AtomicBool::new(false),
            catalog_terminator,
            profile,
            creator: std::process::id(),
            visibility,
            previous: previous::PreviousState::new(
                if previous {
                    if flags & RDONLY != 0 {
                        previous::READONLY
                    } else {
                        previous::PENDING
                    }
                } else {
                    previous::NORMAL
                },
                pending,
            ),
            _environment_lease: environment_lease,
            coordination,
            external,
            #[cfg(any(target_os = "linux", target_os = "macos", windows))]
            fixed_map,
            #[cfg(any(target_os = "linux", target_os = "macos", windows))]
            fixed_declaration,
        });
        if let Some(minimum) = map_size::minimum(&shared, &initial)? {
            #[cfg(any(target_os = "linux", target_os = "macos", windows))]
            if let Some(fixed) = &shared.fixed_map
                && minimum > fixed.info().map_err(code)?.capacity
            {
                return Err(libc::ENOTSUP);
            }
            env.mapsize = env.mapsize.max(minimum);
        }
        if let Some(context) = &shared.transform {
            context.raise_decoded_byte_limit(crypto::decoded_limit(env.mapsize));
        }
        env.shared = Some(shared);
        env.path = Some(unsafe { CStr::from_ptr(path) }.to_owned());
        env.flags.store(flags, std::sync::atomic::Ordering::Release);
        Ok(())
    })
}
#[unsafe(no_mangle)]
pub unsafe extern "C" fn mdb_env_close(env: *mut MDB_env) {
    if !env.is_null() {
        let _ = catch_unwind(AssertUnwindSafe(|| drop(unsafe { Box::from_raw(env) })));
    }
}
#[unsafe(no_mangle)]
pub unsafe extern "C" fn mdb_env_set_mapsize(env: *mut MDB_env, size: usize) -> c_int {
    boundary(|| {
        let e = unsafe { get_mut(env)? };
        if let Some(shared) = &e.shared
            && shared.writer_thread.lock().map_err(|_| BAD_TXN)?.is_some()
        {
            return Err(libc::EINVAL);
        }
        #[cfg(any(target_os = "linux", target_os = "macos", windows))]
        if let Some(fixed) = e
            .shared
            .as_ref()
            .and_then(|shared| shared.fixed_map.as_ref())
            && size != 0
            && size != fixed.info().map_err(code)?.capacity
        {
            return Err(libc::ENOTSUP);
        }
        let mut selected = if size != 0 {
            size
        } else if let Some(shared) = &e.shared {
            map_size::read(&shared.directory)?.unwrap_or(e.mapsize)
        } else {
            e.mapsize
        };
        if let Some(shared) = &e.shared {
            let read = shared.open_snapshot(true)?;
            if let Some(minimum) = map_size::minimum(shared, &read)? {
                selected = selected.max(minimum);
            }
            #[cfg(any(target_os = "linux", target_os = "macos", windows))]
            if let Some(fixed) = &shared.fixed_map
                && selected > fixed.info().map_err(code)?.capacity
            {
                return Err(libc::ENOTSUP);
            }
        }
        e.mapsize = selected;
        if size != 0 {
            e.mapsize_explicit = true;
        } else if e.shared.is_none() {
            e.mapsize_explicit = false;
        }
        if let Some(context) = e
            .shared
            .as_ref()
            .and_then(|shared| shared.transform.as_ref())
        {
            context.raise_decoded_byte_limit(crypto::decoded_limit(e.mapsize));
        }
        Ok(())
    })
}
#[unsafe(no_mangle)]
pub unsafe extern "C" fn mdb_env_set_maxdbs(env: *mut MDB_env, count: u32) -> c_int {
    boundary(|| {
        let e = unsafe { get_mut(env)? };
        if e.shared.is_some() {
            return Err(libc::EINVAL);
        }
        if count > dbi_state::MAX_USER_DATABASES {
            return Err(libc::EINVAL);
        }
        e.maxdbs = count;
        Ok(())
    })
}
#[unsafe(no_mangle)]
pub unsafe extern "C" fn mdb_env_set_maxreaders(env: *mut MDB_env, count: u32) -> c_int {
    boundary(|| {
        let e = unsafe { get_mut(env)? };
        if e.shared.is_some() || count == 0 {
            return Err(libc::EINVAL);
        }
        e.maxreaders = count;
        Ok(())
    })
}
#[unsafe(no_mangle)]
pub unsafe extern "C" fn mdb_env_get_maxreaders(env: *mut MDB_env, out: *mut u32) -> c_int {
    boundary(|| {
        *unsafe { get_mut(out)? } = unsafe { get_ref(env)? }.maxreaders;
        Ok(())
    })
}
#[unsafe(no_mangle)]
pub unsafe extern "C" fn mdb_env_get_maxkeysize(env: *mut MDB_env) -> c_int {
    match unsafe { get_ref(env) } {
        Ok(env) => env.shared.as_ref().map_or_else(
            || {
                if alias::Profile::select(None, std::env::var("ULMDB_LMDB_ABI").ok().as_deref())
                    .catalog_terminator()
                {
                    0
                } else {
                    511
                }
            },
            |shared| shared.max_key_size(),
        ) as c_int,
        Err(_) => 0,
    }
}
#[unsafe(no_mangle)]
pub unsafe extern "C" fn mdb_env_get_flags(env: *mut MDB_env, out: *mut u32) -> c_int {
    boundary(|| {
        let env = unsafe { get_ref(env)? };
        let active = env
            .shared
            .as_ref()
            .map_or(env.flags() & visibility::PREVIOUS != 0, |shared| {
                shared.previous.active()
            });
        *unsafe { get_mut(out)? } =
            (env.flags() & !visibility::PREVIOUS) | if active { visibility::PREVIOUS } else { 0 };
        Ok(())
    })
}
#[unsafe(no_mangle)]
pub unsafe extern "C" fn mdb_env_set_flags(env: *mut MDB_env, flags: u32, on: c_int) -> c_int {
    boundary(|| {
        let env = unsafe { get_ref(env)? };
        if flags & !(0x10000 | 0x40000 | 0x100000 | 0x1000000) != 0 {
            return Err(libc::EINVAL);
        }
        if on != 0 {
            env.flags
                .fetch_or(flags, std::sync::atomic::Ordering::AcqRel);
        } else {
            env.flags
                .fetch_and(!flags, std::sync::atomic::Ordering::AcqRel);
        }
        Ok(())
    })
}
#[unsafe(no_mangle)]
pub unsafe extern "C" fn mdb_env_get_path(env: *mut MDB_env, out: *mut *const c_char) -> c_int {
    boundary(|| {
        *unsafe { get_mut(out)? } = unsafe { get_ref(env)? }
            .path
            .as_ref()
            .map_or(ptr::null(), |path| path.as_ptr());
        Ok(())
    })
}
#[unsafe(no_mangle)]
pub unsafe extern "C" fn mdb_env_get_fd(env: *mut MDB_env, out: *mut FileHandle) -> c_int {
    boundary(|| {
        *unsafe { get_mut(out)? } = unsafe { get_ref(env)? }.data_file.as_ref().map_or(
            ulmdb_platform::INVALID_FILE_HANDLE,
            ulmdb_platform::raw_file_handle,
        );
        Ok(())
    })
}
#[unsafe(no_mangle)]
pub unsafe extern "C" fn mdb_env_set_userctx(env: *mut MDB_env, context: *mut c_void) -> c_int {
    boundary(|| {
        unsafe { get_mut(env)? }.userctx = context;
        Ok(())
    })
}
#[unsafe(no_mangle)]
pub unsafe extern "C" fn mdb_env_get_userctx(env: *mut MDB_env) -> *mut c_void {
    unsafe { env.as_ref() }.map_or(ptr::null_mut(), |e| e.userctx)
}
#[unsafe(no_mangle)]
pub unsafe extern "C" fn mdb_env_set_assert(env: *mut MDB_env, callback: *mut c_void) -> c_int {
    boundary(|| {
        unsafe { get_mut(env)? }.assert_callback = if callback.is_null() {
            None
        } else {
            Some(unsafe {
                std::mem::transmute::<*mut c_void, unsafe extern "C" fn(*mut MDB_env, *const c_char)>(
                    callback,
                )
            })
        };
        Ok(())
    })
}
#[unsafe(no_mangle)]
pub unsafe extern "C" fn mdb_env_sync(env: *mut MDB_env, force: c_int) -> c_int {
    boundary(|| {
        let env = unsafe { get_ref(env)? };
        let shared = env.shared.as_ref().ok_or(libc::EINVAL)?;
        if env.flags() & RDONLY != 0 {
            return Err(shared.readonly_error());
        }
        shared.sync_selected(force != 0)
    })
}
#[unsafe(no_mangle)]
pub unsafe extern "C" fn mdb_env_info(env: *mut MDB_env, out: *mut MDB_envinfo) -> c_int {
    boundary(|| {
        let e = unsafe { get_ref(env)? };
        let shared = e.shared.as_ref().ok_or(libc::EINVAL)?;
        let stats = previous::metadata_snapshot(shared, shared.previous.active(), false)?.stats();
        *unsafe { get_mut(out)? } = MDB_envinfo {
            me_mapaddr: fixed_map::address(shared)?,
            me_mapsize: e.mapsize,
            me_last_pgno: (stats.segment_bytes / stats.page_size as u64) as usize,
            me_last_txnid: stats.sequence as usize,
            me_maxreaders: e.maxreaders,
            me_numreaders: if e.flags() & 0x400000 != 0 {
                0 // MDB_NOLOCK environments have no LMDB reader table.
            } else {
                process::reader_high_water(e.shared.as_ref().unwrap())?
            },
        };
        Ok(())
    })
}
#[unsafe(no_mangle)]
pub unsafe extern "C" fn mdb_env_stat(env: *mut MDB_env, out: *mut MDB_stat) -> c_int {
    boundary(|| {
        let e = unsafe { get_ref(env)? };
        let shared = e.shared.as_ref().ok_or(libc::EINVAL)?;
        let r = previous::metadata_snapshot(shared, shared.previous.active(), true)?;
        let native = match r.database_stats(MAIN) {
            Ok(stats) => stats,
            Err(Error::MissingDatabase) => ulmdb::DatabaseStats {
                page_size: shared.page_size.get(),
                ..Default::default()
            },
            Err(e) => return Err(code(e)),
        };
        let mut result = stats::mdb_fields(native)?;
        let named = r
            .database_names()
            .map_err(code)?
            .iter()
            .filter(|name| names::is_alias(name) || name.first().is_some_and(|b| *b != 0))
            .count();
        result.ms_entries = result.ms_entries.checked_add(named).ok_or(-30792)?;
        *unsafe { get_mut(out)? } = result;
        Ok(())
    })
}
#[unsafe(no_mangle)]
pub unsafe extern "C" fn mdb_reader_check(env: *mut MDB_env, dead: *mut c_int) -> c_int {
    boundary(|| {
        let e = unsafe { get_ref(env)? };
        let (_, count) = process::readers(e.shared.as_ref().ok_or(libc::EINVAL)?, true)?;
        *unsafe { get_mut(dead)? } = count;
        Ok(())
    })
}
#[unsafe(no_mangle)]
pub unsafe extern "C" fn mdb_reader_list(
    env: *mut MDB_env,
    callback: *mut c_void,
    context: *mut c_void,
) -> c_int {
    boundary(|| {
        if callback.is_null() {
            return Err(libc::EINVAL);
        }
        let e = unsafe { get_ref(env)? };
        let (rows, _) = process::readers(e.shared.as_ref().ok_or(libc::EINVAL)?, false)?;
        let callback: unsafe extern "C" fn(*const c_char, *mut c_void) -> c_int =
            unsafe { std::mem::transmute(callback) };
        let header = if rows.is_empty() {
            c"(no active readers)\n"
        } else {
            c"    pid     thread     txnid\n"
        };
        let result = unsafe { callback(header.as_ptr(), context) };
        if result != 0 {
            return Err(result);
        }
        for row in rows {
            let row = std::ffi::CString::new(row).map_err(|_| -30796)?;
            let result = unsafe { callback(row.as_ptr(), context) };
            if result != 0 {
                return Err(result);
            }
        }
        Ok(())
    })
}
#[unsafe(no_mangle)]
pub unsafe extern "C" fn mdb_version(
    major: *mut c_int,
    minor: *mut c_int,
    patch: *mut c_int,
) -> *mut c_char {
    let requested = std::env::var("ULMDB_LMDB_ABI").ok();
    let (api_major, api_minor, api_patch, text) =
        match alias::Profile::select(None, requested.as_deref()) {
            alias::Profile::Legacy31 => (0, 9, 31, c"LMDB 0.9.31 API / ULMDB 0.1.0 adapter"),
            alias::Profile::Legacy36 => (0, 9, 36, c"LMDB 0.9.36 API / ULMDB 0.1.0 adapter"),
            alias::Profile::Modern => (1, 0, 2, c"LMDB 1.0.2 API / ULMDB 0.1.0 adapter"),
        };
    unsafe {
        if let Some(p) = major.as_mut() {
            *p = api_major;
        }
        if let Some(p) = minor.as_mut() {
            *p = api_minor;
        }
        if let Some(p) = patch.as_mut() {
            *p = api_patch;
        }
    }
    text.as_ptr().cast_mut()
}
#[unsafe(no_mangle)]
pub unsafe extern "C" fn mdb_strerror(error: c_int) -> *mut c_char {
    match error {
        0 => c"Successful return".as_ptr().cast_mut(),
        KEYEXIST => c"MDB_KEYEXIST: Key/data pair already exists"
            .as_ptr()
            .cast_mut(),
        NOTFOUND => c"MDB_NOTFOUND: No matching key/data pair found"
            .as_ptr()
            .cast_mut(),
        -30797 => c"MDB_PAGE_NOTFOUND: Requested page was not found"
            .as_ptr()
            .cast_mut(),
        -30794 => c"MDB_VERSION_MISMATCH: Incompatible database format"
            .as_ptr()
            .cast_mut(),
        -30793 => c"MDB_INVALID: Input is not a valid database environment"
            .as_ptr()
            .cast_mut(),
        -30790 => c"MDB_READERS_FULL: Too many reader slots in use"
            .as_ptr()
            .cast_mut(),
        -30789 => c"MDB_TLS_FULL: Thread-local storage limit exceeded"
            .as_ptr()
            .cast_mut(),
        -30788 => c"MDB_TXN_FULL: Transaction resource limit exceeded"
            .as_ptr()
            .cast_mut(),
        -30787 => c"MDB_CURSOR_FULL: Cursor resource limit exceeded"
            .as_ptr()
            .cast_mut(),
        -30786 => c"MDB_PAGE_FULL: Page has no free space".as_ptr().cast_mut(),
        -30785 => c"MDB_MAP_RESIZED: Adopt the published map capacity"
            .as_ptr()
            .cast_mut(),
        -30783 => c"MDB_BAD_RSLOT: Invalid reader slot".as_ptr().cast_mut(),
        -30778 => c"MDB_BAD_CHECKSUM: Checksum verification failed"
            .as_ptr()
            .cast_mut(),
        -30779 => c"MDB_PROBLEM: Unexpected problem - transaction should abort"
            .as_ptr()
            .cast_mut(),
        -30772 => c"MDB_SHORT_WRITE: Write was incomplete".as_ptr().cast_mut(),
        -30771 => c"MDB_ENV_BUSY: Environment is busy".as_ptr().cast_mut(),
        -30777 => c"MDB_CRYPTO_FAIL: Encryption or authentication failed"
            .as_ptr()
            .cast_mut(),
        -30776 => c"MDB_ENV_ENCRYPTION: Environment transform configuration mismatch"
            .as_ptr()
            .cast_mut(),
        -30773 => c"MDB_DBIS_BUSY: Close named database handles before clearing the main database"
            .as_ptr()
            .cast_mut(),
        -30770 => c"MDB_IS_READONLY: Can't write in readonly txn or environment"
            .as_ptr()
            .cast_mut(),
        -30769 => c"MDB_ADDR_BUSY: Fixed mapping address is occupied"
            .as_ptr()
            .cast_mut(),
        CANT_ROLLBACK => c"MDB_CANT_ROLLBACK: Environment can't rollback last transaction"
            .as_ptr()
            .cast_mut(),
        TXN_PENDING => c"MDB_TXN_PENDING: Transaction already prepared, must abort or commit"
            .as_ptr()
            .cast_mut(),
        BAD_TXN => c"MDB_BAD_TXN: Invalid or inactive transaction"
            .as_ptr()
            .cast_mut(),
        BAD_DBI => c"MDB_BAD_DBI: Invalid database handle".as_ptr().cast_mut(),
        BAD_VALSIZE => c"MDB_BAD_VALSIZE: Invalid key or value size"
            .as_ptr()
            .cast_mut(),
        INCOMPATIBLE => c"MDB_INCOMPATIBLE: Database type mismatch"
            .as_ptr()
            .cast_mut(),
        -30796 => c"MDB_CORRUPTED: Database integrity error"
            .as_ptr()
            .cast_mut(),
        -30795 => c"MDB_PANIC: Commit outcome unknown; reopen required"
            .as_ptr()
            .cast_mut(),
        -30792 => c"MDB_MAP_FULL: Resource limit exceeded".as_ptr().cast_mut(),
        -30791 => c"MDB_DBS_FULL: Too many named databases"
            .as_ptr()
            .cast_mut(),
        e if e > 0 => unsafe { libc::strerror(e) },
        _ => c"ULMDB: Unknown error".as_ptr().cast_mut(),
    }
}

#[unsafe(no_mangle)]
pub unsafe extern "C" fn mdb_env_set_pagesize(env: *mut MDB_env, size: c_int) -> c_int {
    boundary(|| {
        let env = unsafe { get_mut(env)? };
        if env.shared.is_some() {
            return Err(libc::EINVAL);
        }
        env.page_size = ulmdb::PageSize::new(size as usize).map_err(|_| libc::EINVAL)?;
        Ok(())
    })
}

#[unsafe(no_mangle)]
pub unsafe extern "C" fn ulmdb_env_set_shared_group(env: *mut MDB_env, gid: u32) -> c_int {
    boundary(|| {
        let env = unsafe { get_mut(env)? };
        if env.shared.is_some() {
            return Err(libc::EINVAL);
        }
        env.shared_group = Some(gid);
        Ok(())
    })
}

#[unsafe(no_mangle)]
pub unsafe extern "C" fn ulmdb_file_access_migrate(
    path: *const c_char,
    mode: u32,
    gid: u32,
) -> c_int {
    boundary(|| {
        let path = unsafe { filesystem_path(path)? };
        let path = path.as_ref();
        let access = if gid == u32::MAX {
            ulmdb_storage::access::FileAccess::requested(mode)
        } else {
            ulmdb_storage::access::FileAccess::shared_group(mode, gid)
        }
        .map_err(code)?;
        let env = Environment::open(path).map_err(code)?;
        env.migrate_file_access(access).map_err(code)?;
        Ok(())
    })
}
