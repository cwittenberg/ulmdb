use super::*;
use std::ffi::CString;

static NEXT: AtomicU64 = AtomicU64::new(1);
struct Fixture { path: PathBuf, env: *mut Env }
impl Fixture {
    fn new() -> Self {
        let path = std::env::temp_dir().join(format!("ulmdb-btree-unit-{}-{}", std::process::id(), NEXT.fetch_add(1, Ordering::Relaxed)));
        let mut env = ptr::null_mut();
        unsafe {
            assert_eq!(ubt_env_create(&mut env), 0);
            assert_eq!(ubt_env_open(env, CString::new(path.as_os_str().as_encoded_bytes()).unwrap().as_ptr(), NOTLS, 0), 0);
        }
        Self { path, env }
    }
    unsafe fn begin(&self, parent: *mut Txn, read: bool) -> *mut Txn {
        let mut txn = ptr::null_mut();
        assert_eq!(unsafe { ubt_txn_begin(self.env, parent, if read { RDONLY } else { 0 }, &mut txn) }, 0);
        txn
    }
    unsafe fn initialize(&self) {
        let w = unsafe { self.begin(ptr::null_mut(), false) };
        let mut dbi = 0;
        assert_eq!(unsafe { ubt_dbi_open(w, c"meta".as_ptr(), CREATE, &mut dbi) }, 0);
        assert_eq!(dbi, 0);
        assert_eq!(unsafe { ubt_dbi_open(w, c"t00000003".as_ptr(), CREATE, &mut dbi) }, 0);
        assert_eq!(dbi, 3);
        assert_eq!(unsafe { put(w, 3, 1, b"old") }, 0);
        assert_eq!(unsafe { ubt_txn_commit(w) }, 0);
    }
}
impl Drop for Fixture {
    fn drop(&mut self) {
        unsafe { if !self.env.is_null() { ubt_env_close(self.env); } }
        let _ = std::fs::remove_dir_all(&self.path);
    }
}
fn encoded(id: i64) -> [u8; 8] { ((id as u64) ^ (1 << 63)).to_be_bytes() }
fn val(bytes: &[u8]) -> Val { Val { size: bytes.len(), data: bytes.as_ptr().cast_mut().cast() } }
unsafe fn put(txn: *mut Txn, dbi: u32, id: i64, bytes: &[u8]) -> i32 {
    let key = encoded(id); let mut key = val(&key); let mut value = val(bytes);
    unsafe { ubt_put(txn, dbi, &mut key, &mut value, 0) }
}
unsafe fn get(txn: *mut Txn, id: i64) -> Result<Vec<u8>> {
    let key = encoded(id); let mut key = val(&key); let mut out = Val { size: 0, data: ptr::null_mut() };
    let rc = unsafe { ubt_get(txn, 3, &mut key, &mut out) };
    if rc != 0 { return Err(rc); }
    Ok(unsafe { bytes(&out) }.unwrap().to_vec())
}
unsafe fn next(cursor: *mut Cursor, op: i32) -> i32 {
    let mut key = Val { size: 0, data: ptr::null_mut() }; let mut value = Val { size: 0, data: ptr::null_mut() };
    unsafe { ubt_cursor_get(cursor, &mut key, &mut value, op) }
}
unsafe fn cursor_row(cursor: *mut Cursor, op: i32, input: Option<i64>) -> Result<(i64, Vec<u8>)> {
    let encoded = encoded(input.unwrap_or(0));
    let mut key = if input.is_some() { val(&encoded) } else { val(&[]) };
    let mut value = val(&[]);
    let rc = unsafe { ubt_cursor_get(cursor, &mut key, &mut value, op) };
    if rc != 0 { return Err(rc); }
    let actual: [u8; 8] = unsafe { bytes(&key)? }.try_into().map_err(|_| IO)?;
    Ok(((u64::from_be_bytes(actual) ^ (1 << 63)) as i64, unsafe { bytes(&value)? }.to_vec()))
}

#[test]
fn native_catalog_delta_child_abort_release_and_real_publication() {
    let f = Fixture::new();
    unsafe {
        f.initialize();
        let w = f.begin(ptr::null_mut(), false);
        let child = f.begin(w, false);
        assert_eq!(put(child, 3, 1, b"discard"), 0);
        let mut new = 0;
        assert_eq!(ubt_dbi_open(child, c"t00000004".as_ptr(), CREATE, &mut new), 0);
        ubt_txn_abort(child);
        assert_eq!(get(w, 1), Ok(b"old".to_vec()));
        assert_eq!(ubt_dbi_open(w, c"t00000004".as_ptr(), 0, &mut new), NOTFOUND);
        let child = f.begin(w, false);
        assert_eq!(put(child, 3, i64::MIN, b"min"), 0);
        assert_eq!(put(child, 3, i64::MAX, b"max"), 0);
        assert_eq!(ubt_txn_commit(child), 0);
        let r = f.begin(ptr::null_mut(), true);
        assert_eq!(get(r, i64::MIN), Err(NOTFOUND));
        ubt_txn_abort(r);
        assert_eq!(ubt_txn_commit(w), 0);
        let r = f.begin(ptr::null_mut(), true);
        assert_eq!(get(r, i64::MIN), Ok(b"min".to_vec()));
        assert_eq!(get(r, i64::MAX), Ok(b"max".to_vec()));
        ubt_txn_abort(r);
    }
}

#[test]
fn native_reset_abort_drop_cells_before_epoch_with_invalid_cursor_handles() {
    let mut f = Fixture::new();
    unsafe {
        f.initialize();
        let r = f.begin(ptr::null_mut(), true);
        let weak = if let State::Read(Some(epoch)) = &(*r).state { Arc::downgrade(epoch) } else { unreachable!() };
        let mut cursor = ptr::null_mut();
        assert_eq!(ubt_cursor_open(r, 3, &mut cursor), 0);
        assert_eq!(next(cursor, 0), 0);
        ubt_txn_reset(r);
        assert!(weak.upgrade().is_none());
        assert!((*cursor).cell.is_none() && (*cursor).txn.is_null());
        assert_eq!(next(cursor, 8), BAD_TXN);
        assert_eq!(ubt_txn_renew(r), 0);
        let w = f.begin(ptr::null_mut(), false);
        assert_eq!(put(w, 3, 1, b"new"), 0);
        assert_eq!(ubt_txn_commit(w), 0);
        assert_eq!(get(r, 1), Ok(b"old".to_vec()));
        ubt_txn_reset(r);
        assert_eq!(ubt_txn_renew(r), 0);
        assert_eq!(get(r, 1), Ok(b"new".to_vec()));
        ubt_cursor_close(cursor);
        assert_eq!(ubt_cursor_open(r, 3, &mut cursor), 0);
        assert_eq!(next(cursor, 0), 0);
        let weak = if let State::Read(Some(epoch)) = &(*r).state { Arc::downgrade(epoch) } else { unreachable!() };
        ubt_txn_abort(r);
        assert!(weak.upgrade().is_none());
        assert_eq!(next(cursor, 4), BAD_TXN);
        ubt_env_close(f.env); f.env = ptr::null_mut();
        // The invalid cursor body retains no epoch, owner or process fence.
        let reopened = open_owner(&f.path, false).unwrap();
        #[cfg(not(feature = "btree-blocks"))]
        assert_eq!(reopened.engine.begin_read().get(DATA, &physical(3, encoded(1))).unwrap(), Some(b"new".as_slice()));
        #[cfg(feature = "btree-blocks")]
        {let read=reopened.engine.begin_read();let rows=blocks::SnapshotRows::open_audited(&read,DATA,3,blocks::Limits::default()).unwrap();assert_eq!(rows.exact(&encoded(1)).unwrap().unwrap().value,b"new");}
        drop(reopened);
        ubt_cursor_close(cursor);
    }
}

#[test]
fn native_read_key_survives_cursor_move_and_close_in_its_real_epoch() {
    let f = Fixture::new();
    unsafe {
        f.initialize();
        let r = f.begin(ptr::null_mut(), true); let mut cursor = ptr::null_mut();
        assert_eq!(ubt_cursor_open(r, 3, &mut cursor), 0);
        let mut key = Val { size: 0, data: ptr::null_mut() }; let mut value = Val { size: 0, data: ptr::null_mut() };
        assert_eq!(ubt_cursor_get(cursor, &mut key, &mut value, 0), 0);
        let retained = Val { size: key.size, data: key.data };
        assert_eq!(ubt_cursor_get(cursor, &mut key, &mut value, 4), 0);
        assert_eq!(next(cursor, 8), NOTFOUND);
        ubt_cursor_close(cursor);
        assert_eq!(bytes(&retained).unwrap(), encoded(1));
        let mut retained = retained;
        assert_eq!(ubt_get(r, 3, &mut retained, &mut value), 0);
        assert_eq!(bytes(&value).unwrap(), b"old");
        ubt_txn_abort(r);
    }
}

#[test]
fn native_readonly_missing_and_foreign_marker_do_not_create_storage() {
    let path = std::env::temp_dir().join(format!("ulmdb-btree-refusal-{}-{}", std::process::id(), NEXT.fetch_add(1, Ordering::Relaxed)));
    unsafe {
        let mut env = ptr::null_mut(); assert_eq!(ubt_env_create(&mut env), 0);
        let name = CString::new(path.as_os_str().as_encoded_bytes()).unwrap();
        assert_eq!(ubt_env_open(env, name.as_ptr(), RDONLY | NOTLS, 0), libc::ENOENT);
        assert!(!path.exists());
        use std::os::unix::fs::DirBuilderExt;
        std::fs::DirBuilder::new().mode(0o700).create(&path).unwrap();
        std::fs::write(path.join(MARKER), b"foreign format").unwrap();
        assert_eq!(ubt_env_open(env, name.as_ptr(), NOTLS, 0), NOTADB);
        assert!(!path.join("SNAPSHOTLOG").exists());
        ubt_env_close(env);
    }
    std::fs::remove_dir_all(path).unwrap();
}

#[test]
fn native_stage_refusal_preserves_actual_delta_before_new_allocation() {
    let f = Fixture::new();
    unsafe {
        f.initialize();
        let w = f.begin(ptr::null_mut(), false); let child = f.begin(w, false);
        let input = vec![0x5a; MAX_ROW];
        for id in 10..13 { assert_eq!(put(child, 3, id, &input), 0); }
        let State::Write(root) = &(*child).state else { unreachable!() };
        let before = (root.borrow().pending_bytes, root.borrow().undo_bytes, root.borrow().pending.len());
        let k = encoded(13); let mut k = val(&k);
        // Quota rejection precedes even reading/copying this unavailable input.
        let mut proposed = Val { size: MAX_ROW, data: ptr::null_mut() };
        assert_eq!(ubt_put(child, 3, &mut k, &mut proposed, 0), TXN_FULL);
        assert_eq!((root.borrow().pending_bytes, root.borrow().undo_bytes, root.borrow().pending.len()), before);
        ubt_txn_abort(child); ubt_txn_abort(w);
    }
}

#[test]
fn native_drop_unwind_detaches_child_before_outer_abort_and_releases_writer() {
    let f = Fixture::new();
    unsafe {
        f.initialize();
        let w = f.begin(ptr::null_mut(), false); let child = f.begin(w, false);
        assert_eq!(put(child, 3, 2, b"uncommitted"), 0);
        let mut cursor = ptr::null_mut();
        assert_eq!(ubt_cursor_open(child, 3, &mut cursor), 0);
        assert_eq!(cursor_row(cursor, 6, None), Ok((2, b"uncommitted".to_vec())));
        ubt_cursor_close(cursor);
        PANIC_DROP.with(|control| control.set(true));
        assert_eq!(ubt_drop(child, 3, 0), IO);
        let State::Write(root) = &(*child).state else { unreachable!() };
        assert!(root.borrow().failed);
        assert!(root.borrow().admitted.borrow().is_empty(), "DROP unwind retained writer admissions");
        assert!(root.borrow().tails.borrow().is_empty(), "DROP unwind retained maximum-key facts");
        assert_eq!(root.borrow().scopes.len(), 2, "internal scope survived unwind");
        ubt_txn_abort(child);
        assert!((*w).child.is_null(), "parent retains consumed child address");
        ubt_txn_abort(w);
        assert!(!(*f.env).owner.as_ref().unwrap().writing.load(Ordering::Acquire));
        let r = f.begin(ptr::null_mut(), true);
        assert_eq!(get(r, 1), Ok(b"old".to_vec()));
        assert_eq!(get(r, 2), Err(NOTFOUND));
        ubt_txn_abort(r);
        let next_writer = f.begin(ptr::null_mut(), false); ubt_txn_abort(next_writer);
    }
}

#[test]
fn native_active_notls_handles_stay_thread_bound_without_consumption() {
    let f = Fixture::new();
    unsafe {
        f.initialize(); let r = f.begin(ptr::null_mut(), true); let raw = r as usize;
        assert_eq!(std::thread::spawn(move || get(raw as *mut Txn, 1)).join().unwrap(), Err(BAD_TXN));
        assert_eq!(get(r, 1), Ok(b"old".to_vec())); ubt_txn_abort(r);
    }
}

#[test]
fn native_validate_keeps_top_scope_thread_poison_and_reset_admission() {
    let f = Fixture::new();
    unsafe {
        f.initialize();
        assert_eq!(ubt_process_id_fast(), std::process::id());
        assert_eq!(ubt_txn_validate(ptr::null_mut()), INVALID);
        let w = f.begin(ptr::null_mut(), false);
        assert_eq!(ubt_txn_validate(w), 0);
        let child = f.begin(w, false);
        assert_eq!(ubt_txn_validate(w), BAD_TXN);
        assert_eq!(ubt_txn_validate(child), 0);
        let raw = child as usize;
        assert_eq!(std::thread::spawn(move || ubt_txn_validate(raw as *mut Txn)).join().unwrap(), BAD_TXN);
        let State::Write(root) = &(*child).state else { unreachable!() };
        root.borrow_mut().failed = true;
        assert_eq!(ubt_txn_validate(child), BAD_TXN);
        ubt_txn_abort(w);
        assert!(!(*f.env).owner.as_ref().unwrap().writing.load(Ordering::Acquire));
        let r = f.begin(ptr::null_mut(), true);
        assert_eq!(ubt_txn_validate(r), 0);
        ubt_txn_reset(r);
        assert_eq!(ubt_txn_validate(r), BAD_TXN);
        assert_eq!(ubt_txn_renew(r), 0);
        assert_eq!(ubt_txn_validate(r), 0);
        ubt_txn_abort(r);
    }
}

#[test]
fn native_fast_process_guard_and_validate_reject_libc_and_raw_fork() {
    let f = Fixture::new();
    unsafe {
        f.initialize();
        let w = f.begin(ptr::null_mut(), false);
        let parent = ubt_process_id_fast();
        for raw in [false, true] {
            for fallback in [false, true] {
                let pid = if raw { libc::syscall(libc::SYS_fork) as i32 } else { libc::fork() };
                assert!(pid >= 0);
                if pid == 0 {
                    libc::alarm(3);
                    if fallback { super::super::PROCESS_GUARD.store(super::super::PID_FALLBACK, Ordering::Release); }
                    let inherited = ubt_process_id_fast();
                    let accepted = inherited == parent || ubt_txn_validate(w) != BAD_TXN;
                    libc::_exit(if accepted { 1 } else { 0 });
                }
                let mut status = 0;
                assert_eq!(libc::waitpid(pid, &mut status, 0), pid);
                assert_eq!(status, 0);
                assert_eq!(ubt_process_id_fast(), parent);
                assert_eq!(ubt_txn_validate(w), 0);
            }
        }
        ubt_txn_abort(w);
    }
}

#[test]
fn native_fallback_fork_and_rawfork_fresh_env_refused_before_locked_registry() {
    let f = Fixture::new();
    for raw in [false, true] {
        let guard = REGISTRY.get().unwrap().lock().unwrap();
        let forbidden = f.path.join(if raw { "raw-child" } else { "fork-child" });
        let name = CString::new(forbidden.as_os_str().as_encoded_bytes()).unwrap();
        unsafe {
            let pid = if raw { libc::syscall(libc::SYS_fork) as i32 } else { libc::fork() };
            assert!(pid >= 0);
            if pid == 0 {
                libc::alarm(3);
                super::super::PROCESS_GUARD.store(super::super::PID_FALLBACK, Ordering::Release);
                let mut created = ptr::null_mut();
                let first = ubt_env_create(&mut created);
                let mut forged = Env { creator: std::process::id(), owner: None, flags: 0 };
                let second = ubt_env_open(&mut forged, name.as_ptr(), NOTLS, 0);
                libc::_exit(if first == BAD_TXN && created.is_null() && second == BAD_TXN { 0 } else { 1 });
            }
            let mut status = 0; assert_eq!(libc::waitpid(pid, &mut status, 0), pid);
            drop(guard);
            assert_eq!(status, 0); assert!(!forbidden.exists());
        }
    }
}

#[test]
fn native_read_admission_is_epoch_local_and_writer_catalog_stays_live() {
    let f = Fixture::new();
    unsafe {
        f.initialize();
        let r = f.begin(ptr::null_mut(), true);
        let mut dbi = 0;
        assert_eq!(ubt_dbi_open(r, c"t00000003".as_ptr(), 0, &mut dbi), 0);
        let mut cursor = ptr::null_mut();
        assert_eq!(ubt_cursor_open(r, dbi, &mut cursor), 0);
        for _ in 0..20 {
            assert_eq!(get(r, 1), Ok(b"old".to_vec()));
            assert_eq!(next(cursor, 0), 0);
            assert_eq!(next(cursor, 8), NOTFOUND);
        }
        let State::Read(Some(epoch)) = &(*r).state else { unreachable!() };
        assert_eq!(epoch.catalog_checks.get(), 1);
        assert_eq!(*epoch.admitted.borrow(), vec![3]);

        let w = f.begin(ptr::null_mut(), false);
        assert_eq!(ubt_drop(w, 3, 1), 0);
        assert_eq!(get(w, 1), Err(BAD_DBI));
        assert_eq!(put(w, 3, 1, b"not admitted"), BAD_DBI);
        assert_eq!(ubt_txn_commit(w), 0);
        // Both admissions refer to the old real catalog, despite publication
        // of a peer DROP. No lookup may switch to the owner's newer head.
        assert_eq!(get(r, 1), Ok(b"old".to_vec()));
        assert_eq!(next(cursor, 4), 0);
        assert_eq!(epoch.catalog_checks.get(), 1);
        ubt_txn_reset(r);
        assert_eq!(next(cursor, 0), BAD_TXN);
        ubt_cursor_close(cursor);
        assert_eq!(ubt_txn_renew(r), 0);
        assert_eq!(get(r, 1), Err(BAD_DBI));
        let mut refused = ptr::null_mut();
        assert_eq!(ubt_cursor_open(r, 3, &mut refused), BAD_DBI);
        assert!(refused.is_null());

        let w = f.begin(ptr::null_mut(), false);
        assert_eq!(ubt_dbi_open(w, c"t00000003".as_ptr(), CREATE, &mut dbi), 0);
        assert_eq!(put(w, 3, 1, b"new"), 0);
        let child = f.begin(w, false);
        assert_eq!(ubt_drop(child, 3, 1), 0);
        assert_eq!(get(child, 1), Err(BAD_DBI));
        ubt_txn_abort(child);
        assert_eq!(get(w, 1), Ok(b"new".to_vec()));
        assert_eq!(ubt_txn_commit(w), 0);
        assert_eq!(get(r, 1), Err(BAD_DBI), "negative admission crossed epochs");
        ubt_txn_reset(r);
        assert_eq!(ubt_txn_renew(r), 0);
        assert_eq!(get(r, 1), Ok(b"new".to_vec()));
        ubt_txn_abort(r);
    }
}

#[test]
fn native_writer_admission_reuses_only_actual_positive_catalog_entries() {
    let f = Fixture::new();
    unsafe {
        f.initialize();
        let w = f.begin(ptr::null_mut(), false);
        let State::Write(root) = &(*w).state else { unreachable!() };
        let root = root.clone();
        for _ in 0..1000 { assert_eq!(get(w, 1), Ok(b"old".to_vec())); }
        assert_eq!(root.borrow().catalog_checks.get(), 1);
        assert_eq!(*root.borrow().admitted.borrow(), vec![3]);
        assert_eq!(put(w, 3, 2, b"actual mutation"), 0);
        assert_eq!(get(w, 1), Ok(b"old".to_vec()));
        assert_eq!(root.borrow().catalog_checks.get(), 1, "data mutation invalidated catalog admission");
        for expected in 2..4 {
            assert_eq!(exists(&*w, 4), Ok(false));
            assert_eq!(root.borrow().catalog_checks.get(), expected);
        }
        root.borrow_mut().put(catalog(4), b"malformed marker").unwrap();
        assert!(root.borrow().admitted.borrow().is_empty());
        for _ in 0..2 { assert_eq!(exists(&*w, 4), Err(NOTADB)); }
        assert!(root.borrow().admitted.borrow().is_empty());
        root.borrow_mut().put(catalog(4), &[0]).unwrap();
        assert_eq!(exists(&*w, 4), Ok(true));
        assert_eq!(*root.borrow().admitted.borrow(), vec![4]);
        ubt_txn_abort(w);
    }
}

#[test]
fn native_writer_admission_tracks_create_drop_reuse_release_and_every_restore() {
    let f = Fixture::new();
    unsafe {
        f.initialize();
        let w = f.begin(ptr::null_mut(), false);
        let State::Write(root) = &(*w).state else { unreachable!() };
        let root = root.clone();
        assert_eq!(get(w, 1), Ok(b"old".to_vec()));
        let initial = root.borrow().catalog_checks.get();
        let child = f.begin(w, false);
        assert_eq!(get(child, 1), Ok(b"old".to_vec()));
        assert_eq!(root.borrow().catalog_checks.get(), initial);
        assert_eq!(ubt_drop(child, 3, 1), 0);
        assert!(root.borrow().admitted.borrow().is_empty());
        assert_eq!(get(child, 1), Err(BAD_DBI));
        assert_eq!(put(child, 3, 1, b"must reject"), BAD_DBI);
        let mut dbi = 0;
        assert_eq!(ubt_dbi_open(child, c"t00000003".as_ptr(), CREATE, &mut dbi), 0);
        assert!(root.borrow().admitted.borrow().is_empty());
        assert_eq!(get(child, 1), Err(NOTFOUND));
        assert_eq!(put(child, 3, 1, b"same DBI new row"), 0);
        let before_release = root.borrow().catalog_checks.get();
        assert_eq!(ubt_txn_commit(child), 0);
        assert_eq!(get(w, 1), Ok(b"same DBI new row".to_vec()));
        assert_eq!(root.borrow().catalog_checks.get(), before_release);
        let child = f.begin(w, false);
        assert_eq!(ubt_drop(child, 3, 1), 0);
        assert_eq!(get(child, 1), Err(BAD_DBI));
        ubt_txn_abort(child);
        assert!(root.borrow().admitted.borrow().is_empty());
        assert_eq!(get(w, 1), Ok(b"same DBI new row".to_vec()));
        let before_empty_restore = root.borrow().catalog_checks.get();
        let child = f.begin(w, false);
        ubt_txn_abort(child); // Empty undo range must also clear admissions.
        assert!(root.borrow().admitted.borrow().is_empty());
        assert_eq!(get(w, 1), Ok(b"same DBI new row".to_vec()));
        assert_eq!(root.borrow().catalog_checks.get(), before_empty_restore + 1);
        drop(root);
        ubt_txn_abort(w);
        let w = f.begin(ptr::null_mut(), false);
        let State::Write(root) = &(*w).state else { unreachable!() };
        assert!(root.borrow().admitted.borrow().is_empty());
        assert_eq!(get(w, 1), Ok(b"old".to_vec()));
        assert_eq!(root.borrow().catalog_checks.get(), 1);
        ubt_txn_abort(w);
    }
}

#[test]
fn native_writer_admission_bound_overflow_uses_real_catalog_and_restores_clear() {
    let f = Fixture::new();
    unsafe {
        f.initialize();
        let w = f.begin(ptr::null_mut(), false);
        let State::Write(root) = &(*w).state else { unreachable!() };
        let root = root.clone();
        for dbi in 100..100 + MAX_DEPTH as u32 + 2 {
            root.borrow_mut().put(catalog(dbi), &[0]).unwrap();
        }
        for dbi in 100..100 + MAX_DEPTH as u32 + 2 { assert_eq!(exists(&*w, dbi), Ok(true)); }
        assert_eq!(root.borrow().admitted.borrow().len(), MAX_DEPTH);
        let checks = root.borrow().catalog_checks.get();
        assert_eq!(checks, MAX_DEPTH + 2);
        for _ in 0..10 {
            assert_eq!(exists(&*w, 100), Ok(true));
            assert_eq!(exists(&*w, 100 + MAX_DEPTH as u32), Ok(true));
        }
        assert_eq!(root.borrow().catalog_checks.get(), checks + 10);
        assert_eq!(root.borrow().admitted.borrow().len(), MAX_DEPTH);
        let child = f.begin(w, false);
        let mut dbi = 0;
        assert_eq!(ubt_dbi_open(child, c"t00001000".as_ptr(), CREATE, &mut dbi), 0);
        assert!(root.borrow().admitted.borrow().is_empty());
        assert_eq!(exists(&*child, 4096), Ok(true));
        ubt_txn_abort(child);
        assert!(root.borrow().admitted.borrow().is_empty());
        assert_eq!(exists(&*w, 4096), Ok(false));
        assert_eq!(exists(&*w, 100), Ok(true));
        drop(root);
        ubt_txn_abort(w);
    }
}

#[test]
fn native_tail_empty_and_ascending_inserts_keep_real_positions_and_owned_views() {
    let f = Fixture::new();
    unsafe {
        f.initialize();
        let w = f.begin(ptr::null_mut(), false); let mut dbi = 0;
        assert_eq!(ubt_dbi_open(w, c"t00000004".as_ptr(), CREATE, &mut dbi), 0);
        let State::Write(root) = &(*w).state else { unreachable!() };
        let root = root.clone();
        let mut cursor = ptr::null_mut();
        assert_eq!(ubt_cursor_open(w, 4, &mut cursor), 0);
        assert_eq!(cursor_row(cursor, 17, Some(-100)), Err(NOTFOUND));
        assert_eq!(root.borrow().tail(4), None, "range miss fabricated an empty-table fact");
        assert_eq!(cursor_row(cursor, 6, None), Err(NOTFOUND));
        assert_eq!(root.borrow().tail(4), Some(Tail::Empty));
        ubt_cursor_close(cursor);
        for id in -100..100 {
            assert_eq!(ubt_cursor_open(w, 4, &mut cursor), 0);
            assert_eq!(cursor_row(cursor, 17, Some(id)), Err(NOTFOUND));
            let expected = if id == -100 { Err(NOTFOUND) } else { Ok((id - 1, encoded(id - 1).to_vec())) };
            assert_eq!(cursor_row(cursor, 6, None), expected);
            assert_eq!(put(w, 4, id, &encoded(id)), 0);
            ubt_cursor_close(cursor);
        }
        assert_eq!(root.borrow().writer_seeks.get(), 2, "ascending inserts repeated actual range/LAST traversals");
        assert_eq!(ubt_cursor_open(w, 4, &mut cursor), 0);
        let mut key = val(&[]); let mut value = val(&[]);
        assert_eq!(ubt_cursor_get(cursor, &mut key, &mut value, 6), 0);
        let mut retained_key = Val { size: key.size, data: key.data };
        let retained_value = Val { size: value.size, data: value.data };
        {
            let root = root.borrow();
            let (actual_key, actual_value) = root.pending.get_key_value(&physical(4, encoded(99))).unwrap();
            assert_eq!(key.data.cast_const().cast::<u8>(), actual_key.as_ptr().add(8));
            assert_eq!(value.data.cast_const().cast::<u8>(), actual_value.as_ref().unwrap().as_ptr());
        }
        assert_eq!(cursor_row(cursor, 0, None), Ok((-100, encoded(-100).to_vec())));
        let before = (*cursor).last;
        assert_eq!(cursor_row(cursor, 17, Some(100)), Err(NOTFOUND));
        assert_eq!((*cursor).last, before, "fast range miss changed the logical cursor position");
        assert_eq!(cursor_row(cursor, 4, None), Ok((-100, encoded(-100).to_vec())));
        ubt_cursor_close(cursor);
        assert_eq!(bytes(&retained_key).unwrap(), encoded(99));
        assert_eq!(bytes(&retained_value).unwrap(), encoded(99));
        assert_eq!(ubt_get(w, 4, &mut retained_key, &mut value), 0);
        assert_eq!(bytes(&value).unwrap(), encoded(99));
        assert_eq!(put(w, 4, 99, b"fresh replacement"), 0);
        assert_eq!(ubt_cursor_open(w, 4, &mut cursor), 0);
        assert_eq!(cursor_row(cursor, 6, None), Ok((99, b"fresh replacement".to_vec())));
        ubt_cursor_close(cursor);
        drop(root); ubt_txn_abort(w);
    }
}

#[test]
fn native_tail_mixed_signed_seeks_sibling_mutations_and_maximum_delete() {
    let f = Fixture::new();
    unsafe {
        f.initialize(); let w = f.begin(ptr::null_mut(), false);
        let mut model = BTreeMap::from([(1, b"old".to_vec())]);
        for id in [i64::MIN, -9, 0, 7, i64::MAX] {
            let value = encoded(id).to_vec(); assert_eq!(put(w, 3, id, &value), 0); model.insert(id, value);
        }
        let State::Write(root) = &(*w).state else { unreachable!() }; let root = root.clone();
        let mut a = ptr::null_mut(); let mut b = ptr::null_mut();
        assert_eq!(ubt_cursor_open(w, 3, &mut a), 0); assert_eq!(ubt_cursor_open(w, 3, &mut b), 0);
        assert_eq!(cursor_row(a, 6, None), Ok((i64::MAX, encoded(i64::MAX).to_vec())));
        let initial = root.borrow().writer_seeks.get();
        assert_eq!(cursor_row(b, 6, None), Ok((i64::MAX, encoded(i64::MAX).to_vec())));
        assert_eq!(root.borrow().writer_seeks.get(), initial);
        for target in [i64::MIN, i64::MIN + 1, -10, -9, -1, 0, 1, 6, 7, i64::MAX] {
            let expected = model.range(target..).next().map(|(id, value)| (*id, value.clone())).ok_or(NOTFOUND);
            assert_eq!(cursor_row(a, 17, Some(target)), expected);
        }
        assert_eq!(ubt_cursor_del(b, 0), 0); model.remove(&i64::MAX);
        assert_eq!(root.borrow().tail(3), None);
        assert_eq!(cursor_row(b, 4, None), Err(NOTFOUND));
        assert_eq!(cursor_row(a, 6, None), Ok((7, encoded(7).to_vec())));
        let before = root.borrow().writer_seeks.get();
        assert_eq!(cursor_row(a, 17, Some(i64::MAX)), Err(NOTFOUND));
        assert_eq!(cursor_row(a, 4, None), Ok((7, encoded(7).to_vec())));
        assert_eq!(root.borrow().writer_seeks.get(), before);
        let incoming = encoded(i64::MAX - 1); let mut incoming = val(&incoming); let mut data = val(b"new maximum");
        assert_eq!(ubt_cursor_put(b, &mut incoming, &mut data, 0), 0);
        model.insert(i64::MAX - 1, b"new maximum".to_vec());
        assert_eq!(cursor_row(a, 6, None), Ok((i64::MAX - 1, b"new maximum".to_vec())));
        assert_eq!(put(w, 3, i64::MAX - 1, b"overwritten maximum"), 0);
        model.insert(i64::MAX - 1, b"overwritten maximum".to_vec());
        assert_eq!(cursor_row(a, 6, None), Ok((i64::MAX - 1, b"overwritten maximum".to_vec())));
        assert_eq!(cursor_row(b, 15, Some(i64::MIN)), Ok((i64::MIN, encoded(i64::MIN).to_vec())));
        assert_eq!(ubt_cursor_del(b, 0), 0); model.remove(&i64::MIN);
        assert_eq!(root.borrow().tail(3), Some(Tail::Maximum(physical(3, encoded(i64::MAX - 1)))));
        assert_eq!(cursor_row(a, 12, None), Ok((7, encoded(7).to_vec())));
        assert_eq!(cursor_row(a, 8, None), Ok((i64::MAX - 1, b"overwritten maximum".to_vec())));
        let mut actual = Vec::new();
        let mut next = cursor_row(a, 0, None);
        while let Ok(row) = next { actual.push(row); next = cursor_row(a, 8, None); }
        assert_eq!(actual, model.into_iter().collect::<Vec<_>>());
        ubt_cursor_close(a); ubt_cursor_close(b); drop(root); ubt_txn_abort(w);
    }
}

#[test]
fn native_tail_release_restore_catalog_and_empty_table_recreation() {
    let f = Fixture::new();
    unsafe {
        f.initialize(); let w = f.begin(ptr::null_mut(), false);
        assert_eq!(put(w, 3, 10, b"ten"), 0);
        let State::Write(root) = &(*w).state else { unreachable!() }; let root = root.clone();
        assert_eq!(writer_last(&root.borrow(), 3).unwrap().unwrap().key, physical(3, encoded(10)));
        let child = f.begin(w, false); assert_eq!(put(child, 3, 20, b"twenty"), 0); assert_eq!(ubt_txn_commit(child), 0);
        assert_eq!(root.borrow().tail(3), Some(Tail::Maximum(physical(3, encoded(20)))));
        let child = f.begin(w, false); assert_eq!(put(child, 3, 30, b"discard"), 0); ubt_txn_abort(child);
        assert!(root.borrow().tails.borrow().is_empty());
        assert_eq!(writer_last(&root.borrow(), 3).unwrap().unwrap().key, physical(3, encoded(20)));
        let child = f.begin(w, false); ubt_txn_abort(child);
        assert!(root.borrow().tails.borrow().is_empty());
        assert_eq!(writer_last(&root.borrow(), 3).unwrap().unwrap().key, physical(3, encoded(20)));
        let mut dbi = 0; assert_eq!(ubt_dbi_open(w, c"t00000004".as_ptr(), CREATE, &mut dbi), 0);
        assert!(root.borrow().tails.borrow().is_empty());
        assert_eq!(ubt_drop(w, 3, 0), 0);
        assert!(writer_last(&root.borrow(), 3).unwrap().is_none());
        assert_eq!(root.borrow().tail(3), Some(Tail::Empty));
        assert_eq!(put(w, 3, i64::MIN, b"actual new row"), 0);
        assert_eq!(writer_last(&root.borrow(), 3).unwrap().unwrap().key, physical(3, encoded(i64::MIN)));
        assert_eq!(ubt_drop(w, 3, 1), 0);
        assert!(root.borrow().tails.borrow().is_empty());
        assert_eq!(ubt_dbi_open(w, c"t00000003".as_ptr(), CREATE, &mut dbi), 0);
        assert!(writer_last(&root.borrow(), 3).unwrap().is_none());
        drop(root); ubt_txn_abort(w);
    }
}

#[test]
fn native_tail_bound_overflow_keeps_actual_last_and_range_paths() {
    let f = Fixture::new();
    unsafe {
        f.initialize(); let w = f.begin(ptr::null_mut(), false);
        let State::Write(root) = &(*w).state else { unreachable!() }; let root = root.clone();
        for dbi in 100..100 + MAX_DEPTH as u32 + 2 { root.borrow_mut().put(catalog(dbi), &[0]).unwrap(); }
        for dbi in 100..100 + MAX_DEPTH as u32 + 2 {
            let mut cursor = ptr::null_mut(); assert_eq!(ubt_cursor_open(w, dbi, &mut cursor), 0);
            assert_eq!(cursor_row(cursor, 6, None), Err(NOTFOUND)); ubt_cursor_close(cursor);
        }
        assert_eq!(root.borrow().tails.borrow().len(), MAX_DEPTH);
        let before = root.borrow().writer_seeks.get();
        for _ in 0..5 {
            let mut cursor = ptr::null_mut(); assert_eq!(ubt_cursor_open(w, 100 + MAX_DEPTH as u32, &mut cursor), 0);
            assert_eq!(cursor_row(cursor, 6, None), Err(NOTFOUND));
            assert_eq!(cursor_row(cursor, 17, Some(0)), Err(NOTFOUND)); ubt_cursor_close(cursor);
        }
        assert_eq!(root.borrow().writer_seeks.get(), before + 10);
        assert_eq!(root.borrow().tails.borrow().len(), MAX_DEPTH);
        drop(root); ubt_txn_abort(w);
    }
}

#[test]
fn native_prepared_stage_counts_once_and_keeps_unapplied_state_unchanged() {
    let f = Fixture::new();
    unsafe {
        f.initialize(); let w = f.begin(ptr::null_mut(), false);
        let State::Write(root) = &(*w).state else { unreachable!() }; let root = root.clone();
        for id in 2..1002 { assert_eq!(put(w, 3, id, b"row"), 0); }
        assert_eq!(root.borrow().admissions.get(), 1000);
        root.borrow_mut().put(physical(3, encoded(2000)), b"direct").unwrap();
        root.borrow_mut().stage(physical(3, encoded(2000)), None).unwrap();
        assert_eq!(root.borrow().admissions.get(), 1002);
        assert_eq!(writer_last(&root.borrow(), 3).unwrap().unwrap().key, physical(3, encoded(1001)));
        let before = (root.borrow().pending_bytes, root.borrow().undo_bytes, root.borrow().pending.len(), root.borrow().tail(3));
        {
            let mut root = root.borrow_mut();
            let prepared = root.prepare_stage(physical(3, encoded(5000)), Some(3)).unwrap();
            drop(prepared);
        }
        assert_eq!(root.borrow_mut().prepare_stage(physical(3, encoded(5000)), Some(3)).unwrap().apply(Some(vec![0; 2])), Err(INVALID));
        assert_eq!((root.borrow().pending_bytes, root.borrow().undo_bytes, root.borrow().pending.len(), root.borrow().tail(3)), before);
        assert!(!root.borrow().failed);
        for flags in [0x10, 0x10010] {
            let k = encoded(1); let mut k = val(&k);
            let mut rejected = Val { size: MAX_ROW + 1, data: ptr::null_mut() };
            let count = root.borrow().admissions.get();
            assert_eq!(ubt_put(w, 3, &mut k, &mut rejected, flags), KEYEXIST);
            assert_eq!(bytes(&rejected).unwrap(), b"old");
            assert_eq!(root.borrow().admissions.get(), count);
        }
        let k = encoded(3000); let mut k = val(&k); let mut reserved = Val { size: 4, data: ptr::null_mut() };
        let count = root.borrow().admissions.get();
        assert_eq!(ubt_put(w, 3, &mut k, &mut reserved, 0x10000), 0);
        assert_eq!(root.borrow().admissions.get(), count + 1);
        ptr::copy_nonoverlapping(b"blob".as_ptr(), reserved.data.cast(), 4);
        assert_eq!(get(w, 3000), Ok(b"blob".to_vec()));
        assert_eq!(root.borrow().tail(3), Some(Tail::Maximum(physical(3, encoded(3000)))));
        drop(root); ubt_txn_abort(w);
    }
}

#[test]
fn native_prepared_stage_alias_copy_and_undo_refusal_preserve_error_order() {
    let f = Fixture::new();
    unsafe {
        f.initialize(); let w = f.begin(ptr::null_mut(), false); let child = f.begin(w, false);
        assert_eq!(put(child, 3, 1, b"abcde"), 0);
        let State::Write(root) = &(*child).state else { unreachable!() }; let root = root.clone();
        let k = encoded(1); let mut k = val(&k); let mut existing = val(&[]);
        assert_eq!(ubt_get(child, 3, &mut k, &mut existing), 0);
        let mut aliased = Val { size: 3, data: existing.data.cast::<u8>().add(1).cast() };
        let count = root.borrow().admissions.get();
        assert_eq!(ubt_put(child, 3, &mut k, &mut aliased, 0), 0);
        assert_eq!(root.borrow().admissions.get(), count + 1);
        assert_eq!(get(child, 1), Ok(b"bcd".to_vec()));
        assert_eq!(writer_last(&root.borrow(), 3).unwrap().unwrap().key, physical(3, encoded(1)));
        let before = (root.borrow().pending_bytes, root.borrow().undo_bytes, root.borrow().undo.len(), root.borrow().tail(3));
        root.borrow().undo_reserve.set(usize::MAX); // Deterministic real try_reserve capacity refusal.
        let mut invalid = Val { size: 3, data: ptr::null_mut() };
        assert_eq!(ubt_put(child, 3, &mut k, &mut invalid, 0), INVALID);
        let mut valid = val(b"xyz");
        assert_eq!(ubt_put(child, 3, &mut k, &mut valid, 0), TXN_FULL);
        assert_eq!((root.borrow().pending_bytes, root.borrow().undo_bytes, root.borrow().undo.len(), root.borrow().tail(3)), before);
        assert_eq!(get(child, 1), Ok(b"bcd".to_vec()));
        assert!(!root.borrow().failed);
        root.borrow().undo_reserve.set(1);
        assert_eq!(put(child, 3, 1, b"fresh"), 0);
        ubt_txn_abort(child);
        assert_eq!(get(w, 1), Ok(b"old".to_vec()));
        assert!(root.borrow().tails.borrow().is_empty());
        drop(root); ubt_txn_abort(w);
    }
}

#[test]
fn native_prepared_stage_keeps_old_buffer_physical_peak_quota() {
    let f = Fixture::new();
    unsafe {
        f.initialize(); let w = f.begin(ptr::null_mut(), false);
        let input = vec![0xa5; MAX_ROW];
        for id in 10..13 { assert_eq!(put(w, 3, id, &input), 0); }
        let State::Write(root) = &(*w).state else { unreachable!() }; let root = root.clone();
        let before = (root.borrow().pending_bytes, root.borrow().undo_bytes, root.borrow().pending.len());
        let k = encoded(10); let mut k = val(&k); let mut aliased = val(&[]);
        assert_eq!(ubt_get(w, 3, &mut k, &mut aliased), 0);
        assert_eq!(aliased.size, MAX_ROW);
        // Replacing a staged value preserves logical size, but copying its
        // replacement while the old buffer exists would exceed the peak cap.
        assert_eq!(ubt_put(w, 3, &mut k, &mut aliased, 0), TXN_FULL);
        let mut unavailable = Val { size: MAX_ROW, data: ptr::null_mut() };
        assert_eq!(ubt_put(w, 3, &mut k, &mut unavailable, 0), TXN_FULL);
        assert_eq!((root.borrow().pending_bytes, root.borrow().undo_bytes, root.borrow().pending.len()), before);
        assert_eq!(bytes(&aliased).unwrap().first(), Some(&0xa5));
        assert_eq!(bytes(&aliased).unwrap().last(), Some(&0xa5));
        assert!(!root.borrow().failed);
        drop(root); ubt_txn_abort(w);
    }
}

#[cfg(feature = "btree-read-hash")]
#[test]
fn native_read_hash_threshold_borrowed_values_and_epoch_release() {
    let f = Fixture::new();
    unsafe {
        f.initialize();
        let w = f.begin(ptr::null_mut(), false);
        assert_eq!(put(w, 3, i64::MIN, b"min"), 0);
        assert_eq!(put(w, 3, i64::MAX, b"max"), 0);
        assert_eq!(ubt_txn_commit(w), 0);
        let r = f.begin(ptr::null_mut(), true);
        let State::Read(Some(epoch)) = &(*r).state else { unreachable!() };
        let weak = Arc::downgrade(epoch);
        for _ in 0..64 { assert_eq!(get(r, 1), Ok(b"old".to_vec())); }
        assert_eq!(epoch.read.with_dependent(|_, lookup| lookup.stats()).4, 0);
        assert_eq!(get(r, 1), Ok(b"old".to_vec()));
        let stats = epoch.read.with_dependent(|_, lookup| lookup.stats());
        assert_eq!(stats.0, 3); assert_eq!(stats.4, 1);
        let key = encoded(1); let mut key = val(&key); let mut retained = val(&[]);
        assert_eq!(ubt_get(r, 3, &mut key, &mut retained), 0);
        #[cfg(not(feature = "btree-blocks"))]
        assert_eq!(retained.data.cast_const().cast::<u8>(), epoch.read.get(DATA, &physical(3, encoded(1))).unwrap().unwrap().as_ptr());
        #[cfg(feature = "btree-blocks")]
        assert_eq!(retained.data.cast_const().cast::<u8>(), base_value(epoch,&physical(3,encoded(1))).unwrap().unwrap().as_ptr());
        assert_eq!(get(r, i64::MIN), Ok(b"min".to_vec()));
        assert_eq!(get(r, i64::MAX), Ok(b"max".to_vec()));
        assert_eq!(get(r, 0), Err(NOTFOUND));
        let raw = r as usize;
        assert_eq!(std::thread::spawn(move || get(raw as *mut Txn, 1)).join().unwrap(), Err(BAD_TXN));
        let w = f.begin(ptr::null_mut(), false);
        assert_eq!(put(w, 3, 1, b"new"), 0); assert_eq!(ubt_txn_commit(w), 0);
        assert_eq!(get(r, 1), Ok(b"old".to_vec()));
        assert_eq!(bytes(&retained).unwrap(), b"old");
        ubt_txn_reset(r);
        assert!(weak.upgrade().is_none(), "index retained an independent read/owner");
        assert_eq!(ubt_txn_renew(r), 0);
        let State::Read(Some(epoch)) = &(*r).state else { unreachable!() };
        assert_eq!(epoch.read.with_dependent(|_, lookup| lookup.stats()), (0, 0, 0, 0, 0));
        for _ in 0..65 { assert_eq!(get(r, 1), Ok(b"new".to_vec())); }
        assert_eq!(epoch.read.with_dependent(|_, lookup| lookup.stats()).4, 1);
        ubt_txn_abort(r);
    }
}

#[cfg(feature = "btree-read-hash")]
#[test]
fn native_read_hash_aggregate_budgets_fall_back_without_query_failure() {
    let f = Fixture::new();
    unsafe {
        f.initialize();
        let w = f.begin(ptr::null_mut(), false); let mut dbi = 0;
        assert_eq!(ubt_dbi_open(w, c"t00000004".as_ptr(), CREATE, &mut dbi), 0);
        assert_eq!(put(w, 4, 1, b"new"), 0); assert_eq!(ubt_txn_commit(w), 0);
        for limits in [(1, 8 << 20, 64 << 20, 1024), (65_536, 600, 64 << 20, 1024),
                       (65_536, 8 << 20, 4, 1024), (65_536, 8 << 20, 64 << 20, 1)] {
            let r = f.begin(ptr::null_mut(), true);
            let State::Read(Some(epoch)) = &(*r).state else { unreachable!() };
            epoch.read.with_dependent(|_, lookup| lookup.limits(limits.0, limits.1, limits.2, limits.3));
            for _ in 0..65 { assert_eq!(get(r, 1), Ok(b"old".to_vec())); }
            for _ in 0..70 {
                let key = encoded(1); let mut key = val(&key); let mut value = val(&[]);
                assert_eq!(ubt_get(r, 4, &mut key, &mut value), 0);
                assert_eq!(bytes(&value).unwrap(), b"new");
            }
            let stats = epoch.read.with_dependent(|_, lookup| lookup.stats());
            assert_eq!(stats.4, 1); assert!(stats.0 <= limits.0 && stats.1 <= limits.1 && stats.2 <= limits.2 && stats.3 <= limits.3);
            ubt_txn_abort(r);
        }
    }
}

#[cfg(feature = "btree-read-hash")]
#[test]
fn native_read_hash_corruption_propagates_instead_of_falling_back() {
    let f = Fixture::new();
    unsafe {
        f.initialize();
        let owner = (*f.env).owner.as_ref().unwrap();
        let mut malformed = physical(3, encoded(2)).to_vec();
        #[cfg(not(feature = "btree-blocks"))]
        malformed.push(1); // Wrong size for the raw-v1 profile.
        #[cfg(feature = "btree-blocks")]
        malformed.push(2); // Unknown physical tag for the grouped profile.
        let mut w = owner.engine.begin_write().unwrap();
        w.put(DATA, &malformed, b"invalid logical key").unwrap(); w.commit().unwrap();
        let r = f.begin(ptr::null_mut(), true);
        #[cfg(not(feature = "btree-blocks"))]
        for _ in 0..64 { assert_eq!(get(r, 1), Ok(b"old".to_vec())); }
        // Grouped snapshots perform full audit before even the ordered lookup.
        assert_eq!(get(r, 1), Err(IO));
        assert_eq!(get(r, 1), Err(IO), "corruption became ordered fallback on retry");
        let State::Read(Some(epoch)) = &(*r).state else { unreachable!() };
        assert_eq!(epoch.read.with_dependent(|_, lookup| lookup.stats()).4, 0);
        ubt_txn_abort(r);
    }
}

#[cfg(feature = "btree-blocks")]
#[path = "blocks_native_tests.rs"]
mod blocks_native;
