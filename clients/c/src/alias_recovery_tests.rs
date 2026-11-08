//! Public C journeys for rollback-owned slot identities and bounded metadata.
use super::*;
use std::ffi::CString;

struct Fixture {
    env: *mut MDB_env,
    path: std::path::PathBuf,
}
impl Fixture {
    fn new() -> Self {
        Self::with_maxdbs(4)
    }
    fn with_maxdbs(maxdbs: u32) -> Self {
        let path = std::env::temp_dir().join(format!(
            "ulmdb-alias-recovery-{}-{}",
            std::process::id(),
            ulmdb_storage::new_identity().unwrap()
        ));
        let name = CString::new(path.to_str().unwrap()).unwrap();
        let mut env = ptr::null_mut();
        unsafe {
            assert_eq!(environment::mdb_env_create(&mut env), 0);
            assert_eq!(environment::mdb_env_set_maxdbs(env, maxdbs), 0);
            assert_eq!(environment::mdb_env_open(env, name.as_ptr(), 0, 0o600), 0);
        }
        Self { env, path }
    }
    fn begin(&self, parent: *mut MDB_txn, flags: u32) -> *mut MDB_txn {
        let mut txn = ptr::null_mut();
        assert_eq!(
            unsafe { transaction::mdb_txn_begin(self.env, parent, flags, &mut txn) },
            0
        );
        txn
    }
    fn shared(&self) -> &Shared {
        unsafe { (*self.env).shared.as_ref().unwrap() }
    }
    fn seed(&self) -> (u32, u32) {
        let txn = self.begin(ptr::null_mut(), 0);
        let a = open(txn, c"a", CREATE);
        let b = open(txn, c"b", CREATE);
        put(txn, a, b"A");
        put(txn, b, b"B");
        assert_eq!(unsafe { transaction::mdb_txn_commit(txn) }, 0);
        unsafe { operations::mdb_dbi_close(self.env, b) };
        (a, b)
    }
}
impl Drop for Fixture {
    fn drop(&mut self) {
        unsafe { environment::mdb_env_close(self.env) };
        let _ = std::fs::remove_dir_all(&self.path);
    }
}
fn open(txn: *mut MDB_txn, name: &CStr, flags: u32) -> u32 {
    let mut slot = u32::MAX;
    assert_eq!(
        unsafe { operations::mdb_dbi_open(txn, name.as_ptr(), flags, &mut slot) },
        0
    );
    slot
}
fn put(txn: *mut MDB_txn, slot: u32, contents: &[u8]) {
    let mut key = MDB_val {
        mv_size: 1,
        mv_data: b"k".as_ptr().cast_mut().cast(),
    };
    let mut value = MDB_val {
        mv_size: contents.len(),
        mv_data: contents.as_ptr().cast_mut().cast(),
    };
    assert_eq!(
        unsafe { operations::mdb_put(txn, slot, &mut key, &mut value, 0) },
        0
    );
}
fn value(txn: *mut MDB_txn, slot: u32) -> Vec<u8> {
    let mut key = MDB_val {
        mv_size: 1,
        mv_data: b"k".as_ptr().cast_mut().cast(),
    };
    let mut value = MDB_val {
        mv_size: 0,
        mv_data: ptr::null_mut(),
    };
    assert_eq!(
        unsafe { operations::mdb_get(txn, slot, &mut key, &mut value) },
        0
    );
    unsafe { std::slice::from_raw_parts(value.mv_data.cast::<u8>(), value.mv_size) }.to_vec()
}

#[test]
fn deleted_slots_cannot_retarget_parent_or_root_handles_after_abort() {
    for nested in [true, false] {
        let fixture = Fixture::new();
        let (a, _) = fixture.seed();
        let parent = fixture.begin(ptr::null_mut(), 0);
        let txn = if nested {
            fixture.begin(parent, 0)
        } else {
            parent
        };
        assert_eq!(unsafe { operations::mdb_drop(txn, a, 1) }, 0);
        let id = *fixture
            .shared()
            .aliases
            .lock()
            .unwrap()
            .reserved
            .get(&a)
            .unwrap();
        unsafe { operations::mdb_dbi_close(fixture.env, a) };
        assert_eq!(
            fixture.shared().aliases.lock().unwrap().reserved.get(&a),
            Some(&id)
        );
        let b = open(txn, c"b", 0);
        assert_ne!(a, b, "a closed rollback-owned slot is quarantined");
        assert_eq!(value(txn, b), b"B");
        unsafe { transaction::mdb_txn_abort(txn) };
        let restored = if nested {
            parent
        } else {
            fixture.begin(ptr::null_mut(), 0)
        };
        assert_eq!(value(restored, a), b"A");
        assert!(fixture.shared().aliases.lock().unwrap().reserved.is_empty());
        unsafe { transaction::mdb_txn_abort(restored) };
    }
}

#[test]
fn child_commit_keeps_reservations_owned_until_outer_abort_and_created_abort_releases_them() {
    let fixture = Fixture::new();
    let (a, _) = fixture.seed();
    let parent = fixture.begin(ptr::null_mut(), 0);
    let child = fixture.begin(parent, 0);
    assert_eq!(unsafe { operations::mdb_drop(child, a, 1) }, 0);
    assert_ne!(open(child, c"b", 0), a);
    assert_eq!(unsafe { transaction::mdb_txn_commit(child) }, 0);
    assert!(
        fixture
            .shared()
            .aliases
            .lock()
            .unwrap()
            .reserved
            .contains_key(&a)
    );
    unsafe { transaction::mdb_txn_abort(parent) };
    let txn = fixture.begin(ptr::null_mut(), 0);
    assert_eq!(value(txn, a), b"A");
    let created = open(txn, c"created", CREATE);
    assert_eq!(unsafe { operations::mdb_drop(txn, created, 1) }, 0);
    unsafe { transaction::mdb_txn_abort(txn) };
    assert!(fixture.shared().aliases.lock().unwrap().reserved.is_empty());
    assert!(
        fixture
            .shared()
            .handles
            .lock()
            .unwrap()
            .get(created as usize)
            .unwrap()
            .is_none()
    );
}

unsafe extern "C" fn forward(a: *const MDB_val, b: *const MDB_val) -> c_int {
    unsafe { bytes(a).unwrap().cmp(bytes(b).unwrap()) as c_int }
}
unsafe extern "C" fn reverse(a: *const MDB_val, b: *const MDB_val) -> c_int {
    unsafe { bytes(b).unwrap().cmp(bytes(a).unwrap()) as c_int }
}

#[test]
fn restoring_a_dormant_alias_does_not_overwrite_another_active_alias_callbacks() {
    let fixture = Fixture::new();
    let (a, _) = fixture.seed();
    let writer = fixture.begin(ptr::null_mut(), 0);
    assert_eq!(
        unsafe { operations::mdb_set_compare(writer, a, forward as *mut c_void) },
        0
    );
    assert_eq!(unsafe { operations::mdb_drop(writer, a, 1) }, 0);
    let reader = fixture.begin(ptr::null_mut(), RDONLY);
    let active = open(reader, c"a", 0);
    assert_ne!(active, a);
    assert_eq!(
        unsafe { operations::mdb_set_compare(reader, active, reverse as *mut c_void) },
        0
    );
    assert_eq!(unsafe { transaction::mdb_txn_commit(reader) }, 0);
    unsafe { transaction::mdb_txn_abort(writer) };
    let id = fixture.shared().handles.lock().unwrap()[active as usize]
        .as_ref()
        .unwrap()
        .id();
    assert!(
        fixture
            .shared()
            .aliases
            .lock()
            .unwrap()
            .dormant
            .contains(&a)
    );
    assert_eq!(
        fixture.shared().options.lock().unwrap()[&id]
            .key_cmp
            .map(|callback| callback as usize),
        Some(reverse as *const () as usize)
    );
    unsafe { operations::mdb_dbi_close(fixture.env, a) };
    assert!(fixture.shared().options.lock().unwrap().contains_key(&id));
    unsafe { operations::mdb_dbi_close(fixture.env, active) };
    assert!(!fixture.shared().options.lock().unwrap().contains_key(&id));
}

#[test]
fn catalog_enumeration_does_not_accumulate_options_for_unexported_identities() {
    let fixture = Fixture::new();
    for _ in 0..32 {
        let txn = fixture.begin(ptr::null_mut(), 0);
        let named = open(txn, c"integer", CREATE | 8);
        assert_eq!(unsafe { transaction::mdb_txn_commit(txn) }, 0);
        unsafe { operations::mdb_dbi_close(fixture.env, named) };
        let txn = fixture.begin(ptr::null_mut(), 0);
        let mut main = 0;
        assert_eq!(
            unsafe { operations::mdb_dbi_open(txn, ptr::null(), 0, &mut main) },
            0
        );
        let mut stats = std::mem::MaybeUninit::<MDB_stat>::uninit();
        assert_eq!(
            unsafe { operations::mdb_stat(txn, main, stats.as_mut_ptr()) },
            0
        );
        assert!(fixture.shared().options.lock().unwrap().is_empty());
        assert_eq!(unsafe { operations::mdb_drop(txn, main, 0) }, 0);
        assert_eq!(unsafe { transaction::mdb_txn_commit(txn) }, 0);
        assert!(fixture.shared().options.lock().unwrap().is_empty());
    }
}

#[test]
fn repeated_open_close_and_reader_reset_keep_per_cycle_alias_ledgers_bounded() {
    let fixture = Fixture::new();
    let (a, _) = fixture.seed();
    unsafe { operations::mdb_dbi_close(fixture.env, a) };
    let reader = fixture.begin(ptr::null_mut(), RDONLY);
    for _ in 0..64 {
        let slot = open(reader, c"a", 0);
        unsafe { operations::mdb_dbi_close(fixture.env, slot) };
        assert_eq!(unsafe { &*reader }.activated_dbis.len(), 1);
    }
    let slot = open(reader, c"a", 0);
    unsafe { transaction::mdb_txn_reset(reader) };
    assert!(unsafe { &*reader }.activated_dbis.is_empty());
    assert!(
        fixture
            .shared()
            .aliases
            .lock()
            .unwrap()
            .dormant
            .contains(&slot)
    );
    for _ in 0..8 {
        assert_eq!(unsafe { transaction::mdb_txn_renew(reader) }, 0);
        assert_eq!(open(reader, c"a", 0), slot);
        unsafe { transaction::mdb_txn_reset(reader) };
        assert!(unsafe { &*reader }.activated_dbis.is_empty());
    }
    unsafe { transaction::mdb_txn_abort(reader) };
}

#[test]
fn absolute_alias_budget_rejects_before_native_creation_and_abort_releases_quarantine() {
    let fixture = Fixture::new();
    let txn = fixture.begin(ptr::null_mut(), 0);
    for index in 2..dbi_state::MAX_ALIAS_SLOTS {
        let name = CString::new(format!("retired-{index}")).unwrap();
        let slot = open(txn, &name, CREATE);
        assert_eq!(slot as usize, index);
        assert_eq!(unsafe { operations::mdb_drop(txn, slot, 1) }, 0);
    }
    let transaction = unsafe { &*txn };
    let before = match transaction.state.as_ref().unwrap() {
        State::Write(writer) => writer.dirty_bytes(),
        _ => unreachable!(),
    };
    let counts = (
        transaction.created_ids.len(),
        transaction.dropped_dbis.len(),
        transaction.closed_dbis.len(),
        transaction.activated_dbis.len(),
    );
    let mut missing = u32::MAX;
    assert_eq!(
        unsafe { operations::mdb_dbi_open(txn, c"over-budget".as_ptr(), CREATE | 8, &mut missing) },
        -30791
    );
    let transaction = unsafe { &*txn };
    assert_eq!(
        counts,
        (
            transaction.created_ids.len(),
            transaction.dropped_dbis.len(),
            transaction.closed_dbis.len(),
            transaction.activated_dbis.len()
        )
    );
    assert!(matches!(
        operations::current_handle(transaction, b"over-budget"),
        Err(Error::MissingDatabase)
    ));
    assert_eq!(
        match transaction.state.as_ref().unwrap() {
            State::Write(writer) => writer.dirty_bytes(),
            _ => unreachable!(),
        },
        before
    );
    assert_eq!(
        fixture.shared().handles.lock().unwrap().len(),
        dbi_state::MAX_ALIAS_SLOTS
    );
    put(txn, 1, b"still-valid-after-admission-denial");
    unsafe { transaction::mdb_txn_abort(txn) };
    assert!(fixture.shared().aliases.lock().unwrap().reserved.is_empty());
    assert!(fixture.shared().options.lock().unwrap().is_empty());
    let txn = fixture.begin(ptr::null_mut(), 0);
    assert_eq!(open(txn, c"after", CREATE), 2);
    unsafe { transaction::mdb_txn_abort(txn) };
}

#[test]
fn old_snapshot_absence_cannot_release_a_current_dormant_binding() {
    let fixture = Fixture::new();
    let seed = fixture.begin(ptr::null_mut(), 0);
    let b = open(seed, c"b", CREATE);
    put(seed, b, b"B");
    assert_eq!(unsafe { transaction::mdb_txn_commit(seed) }, 0);
    unsafe { operations::mdb_dbi_close(fixture.env, b) };
    let old = fixture.begin(ptr::null_mut(), RDONLY);
    let create = fixture.begin(ptr::null_mut(), 0);
    let a = open(create, c"a", CREATE);
    put(create, a, b"A");
    assert_eq!(unsafe { transaction::mdb_txn_commit(create) }, 0);
    let deleting = fixture.begin(ptr::null_mut(), 0);
    assert_eq!(unsafe { operations::mdb_drop(deleting, a, 1) }, 0);
    unsafe { transaction::mdb_txn_abort(deleting) };
    assert!(
        fixture
            .shared()
            .aliases
            .lock()
            .unwrap()
            .dormant
            .contains(&a)
    );
    let old_b = open(old, c"b", 0);
    assert_ne!(old_b, a);
    assert_eq!(value(old, old_b), b"B");
    unsafe { transaction::mdb_txn_abort(old) };
    let current = fixture.begin(ptr::null_mut(), RDONLY);
    assert_eq!(value(current, a), b"A");
    unsafe { transaction::mdb_txn_abort(current) };
}

#[test]
fn detached_dormant_binding_cannot_be_reused_for_a_new_database() {
    let fixture = Fixture::new();
    let (a, _) = fixture.seed();
    let deleting = fixture.begin(ptr::null_mut(), 0);
    assert_eq!(unsafe { operations::mdb_drop(deleting, a, 1) }, 0);
    unsafe { transaction::mdb_txn_abort(deleting) };
    let writer = fixture.begin(ptr::null_mut(), 0);
    assert_eq!(value(writer, a), b"A");
    let mut main = u32::MAX;
    assert_eq!(
        unsafe { operations::mdb_dbi_open(writer, ptr::null(), 0, &mut main) },
        0
    );
    assert_eq!(unsafe { operations::mdb_drop(writer, main, 0) }, 0);
    let b = open(writer, c"new-b", CREATE);
    put(writer, b, b"B");
    assert_ne!(b, a);
    assert_eq!(value(writer, a), b"A");
    unsafe { transaction::mdb_txn_abort(writer) };
}

#[test]
fn cyclic_alias_rebinding_is_compact_in_root_nested_and_private_readers() {
    const NAMES: usize = 100;
    for role in ["root-reader", "nested-writer", "private-reader"] {
        let fixture = Fixture::with_maxdbs(128);
        Arc::get_mut(unsafe { (*fixture.env).shared.as_mut().unwrap() })
            .unwrap()
            .catalog_terminator = true;
        let names: Vec<_> = (0..NAMES)
            .map(|index| CString::new(format!("cycle-{index}")).unwrap())
            .collect();
        let parent = fixture.begin(ptr::null_mut(), 0);
        let seeded: Vec<_> = names
            .iter()
            .map(|name| open(parent, name, CREATE))
            .collect();
        if role == "root-reader" {
            assert_eq!(unsafe { transaction::mdb_txn_commit(parent) }, 0);
        }
        for &slot in &seeded {
            unsafe { operations::mdb_dbi_close(fixture.env, slot) };
        }
        let txn = if role == "root-reader" {
            fixture.begin(ptr::null_mut(), RDONLY)
        } else {
            fixture.begin(parent, if role == "private-reader" { RDONLY } else { 0 })
        };
        let mut distinct = std::collections::BTreeSet::new();
        for rotation in 0..82 {
            let mut current = Vec::new();
            for index in 0..NAMES {
                let name = (index + rotation) % NAMES;
                let slot = open(txn, &names[name], 0);
                distinct.insert((slot, name));
                current.push(slot);
            }
            for slot in current {
                unsafe { operations::mdb_dbi_close(fixture.env, slot) };
            }
            assert!(unsafe { &*txn }.activated_dbis.len() <= NAMES, "{role}");
        }
        assert_eq!(
            distinct.len(),
            8200,
            "same valid journey as the pre-repair growth probe"
        );
        println!(
            "ALIAS_LEDGER role={role} distinct_pairs={} activation_authorities={}",
            distinct.len(),
            unsafe { &*txn }.activated_dbis.len()
        );
        if role == "root-reader" {
            unsafe { transaction::mdb_txn_abort(txn) };
        } else {
            if role == "nested-writer" {
                assert_eq!(unsafe { transaction::mdb_txn_commit(txn) }, 0);
            } else {
                unsafe { transaction::mdb_txn_abort(txn) };
            }
            assert_eq!(unsafe { &*parent }.created_ids.len(), NAMES);
            if role == "private-reader" {
                assert!(
                    unsafe { &*parent }
                        .private_readers
                        .as_ref()
                        .unwrap()
                        .opened_slots()
                        .unwrap()
                        .len()
                        <= NAMES
                );
            }
            let recovered = open(parent, &names[0], 0);
            assert_eq!(
                unsafe { operations::mdb_set_compare(parent, recovered, forward as *mut c_void) },
                0
            );
            unsafe { transaction::mdb_txn_abort(parent) };
            assert!(
                fixture
                    .shared()
                    .handles
                    .lock()
                    .unwrap()
                    .iter()
                    .skip(2)
                    .all(Option::is_none)
            );
            assert!(fixture.shared().options.lock().unwrap().is_empty());
        }
    }
}

#[test]
fn projected_parent_binding_survives_child_delete_abort_without_retargeting_fresh_caps() {
    let fixture = Fixture::new();
    let (a, _) = fixture.seed();
    // A genuine independent native publication replaces the same public name.
    // Native IDs remain monotonic and the old public alias stays globally open.
    let native = Environment::open(&fixture.path).unwrap();
    let mut writer = native.begin_write().unwrap();
    writer.drop_database(b"a").unwrap();
    writer.create_database(b"a", DatabaseKind::Kv).unwrap();
    writer.put(b"a", b"k", b"recreated").unwrap();
    writer.commit().unwrap();
    drop(native);
    let parent = fixture.begin(ptr::null_mut(), 0);
    assert_eq!(value(parent, a), b"recreated");
    let prior = unsafe { &*parent }.dbis.lock().unwrap().bindings.clone();
    assert_eq!(prior.len(), 1);
    let child = fixture.begin(parent, 0);
    assert_eq!(unsafe { operations::mdb_drop(child, a, 1) }, 0);
    unsafe { transaction::mdb_txn_abort(child) };
    {
        let current = unsafe { &*parent }.dbis.lock().unwrap();
        assert_eq!(current.bindings.len(), prior.len());
        for (id, binding) in &prior {
            assert!(Arc::ptr_eq(current.bindings.get(id).unwrap(), binding));
        }
    }
    assert_eq!(value(parent, a), b"recreated");
    put(parent, a, b"parent-private");
    assert_eq!(value(parent, a), b"parent-private");
    unsafe { transaction::mdb_txn_abort(parent) };
    // An unbound recovered closed handle never gains a fresh physical identity.
    let fresh = fixture.begin(ptr::null_mut(), 0);
    assert_eq!(unsafe { &*fresh }.peek_handle(a).unwrap_err(), BAD_DBI);
    let properly_reopened = open(fresh, c"a", 0);
    assert_ne!(properly_reopened, a);
    assert_eq!(value(fresh, properly_reopened), b"recreated");
    unsafe { transaction::mdb_txn_abort(fresh) };
}

// Adapter integrity fault: lose the last C caller's cap without losing the
// DIRTY ledger. This is not a positive oracle for forbidden dirty DBI close.
#[test]
fn missing_dirty_retained_member_fails_before_selection_and_releases_writer() {
    let f = Fixture::new();
    let (a, _) = f.seed();
    let t = f.begin(ptr::null_mut(), 0);
    assert_eq!(value(t, a), b"A");
    assert_eq!(unsafe { operations::mdb_drop(t, 1, 0) }, 0);
    put(t, 1, b"anchor"); // A nonempty MAIN rearms the clean alias's touch.
    put(t, a, b"private");
    let id = unsafe { &*t }.peek_handle(a).unwrap().id();
    let before = std::fs::read(f.path.join("CURRENT")).unwrap();
    let lost = unsafe { &*t }
        .dbis
        .lock()
        .unwrap()
        .bindings
        .remove(&id)
        .unwrap();
    drop(lost);
    assert_eq!(unsafe { transaction::mdb_txn_commit(t) }, BAD_DBI);
    assert_eq!(std::fs::read(f.path.join("CURRENT")).unwrap(), before);
    let fresh = f.begin(ptr::null_mut(), 0);
    assert_eq!(value(fresh, a), b"A");
    unsafe { transaction::mdb_txn_abort(fresh) };
}

#[test]
fn child_clean_member_close_and_abort_restores_parent_retained_authority() {
    let f = Fixture::new();
    let (a, _) = f.seed();
    let parent = f.begin(ptr::null_mut(), 0);
    assert_eq!(value(parent, a), b"A");
    let id = unsafe { &*parent }.peek_handle(a).unwrap().id();
    assert_eq!(unsafe { operations::mdb_drop(parent, 1, 0) }, 0);
    let child = f.begin(parent, 0);
    unsafe { operations::mdb_dbi_close(f.env, a) }; // clean, never used as a DBI again
    let y = open(child, c"child-y", CREATE);
    put(child, y, b"child");
    unsafe { transaction::mdb_txn_abort(child) };
    {
        let p = unsafe { &*parent };
        let s = p.dbis.lock().unwrap();
        let binding = s.bindings.get(&id).unwrap();
        let DataView::Writable(cap) = &binding.view else {
            panic!("writable parent")
        };
        let group = s.retained.get(&binding.native_id()).unwrap();
        let Some(State::Write(writer)) = p.state.as_ref() else {
            panic!("outer writer")
        };
        assert!(writer.retained_database_view(group, cap).is_ok());
        assert_eq!(writer.view_get(cap, b"k").unwrap(), Some(b"A".as_slice()));
    }
    // Closing was global, so use only a proper fresh output from this point.
    let replacement = open(parent, c"a", CREATE);
    put(parent, replacement, b"new");
    assert_eq!(unsafe { transaction::mdb_txn_commit(parent) }, 0);
    let read = f.begin(ptr::null_mut(), RDONLY);
    assert_eq!(value(read, replacement), b"new");
    unsafe { transaction::mdb_txn_abort(read) };
}

unsafe extern "C" fn main_ascii_fold(a: *const MDB_val, b: *const MDB_val) -> c_int {
    let a = unsafe { bytes(a) }.unwrap();
    let b = unsafe { bytes(b) }.unwrap();
    a.iter()
        .map(u8::to_ascii_lowercase)
        .cmp(b.iter().map(u8::to_ascii_lowercase)) as c_int
}
#[test]
fn retained_readonly_descendant_prevents_parent_prune_until_scope_ends() {
    let f = Fixture::new();
    {
        let shared = Arc::get_mut(unsafe { (*f.env).shared.as_mut().unwrap() }).unwrap();
        shared.catalog_terminator = true;
        shared.profile = alias::Profile::Modern;
    }
    let set_main = |t| {
        assert_eq!(
            unsafe { operations::mdb_set_compare(t, 1, main_ascii_fold as *mut c_void) },
            0
        )
    };
    let seed = f.begin(ptr::null_mut(), 0);
    set_main(seed);
    let upper = open(seed, c"X", CREATE);
    put(seed, upper, b"A");
    assert_eq!(unsafe { transaction::mdb_txn_commit(seed) }, 0);
    let parent = f.begin(ptr::null_mut(), 0);
    set_main(parent);
    let lower = open(parent, c"x", 0);
    assert_ne!(lower, upper);
    assert_eq!(value(parent, lower), b"A");
    assert_eq!(unsafe { operations::mdb_drop(parent, upper, 1) }, 0); // closes only upper
    let child = f.begin(parent, RDONLY);
    assert_eq!(value(child, lower), b"A");
    let mut denied = 0;
    assert_eq!(
        unsafe { operations::mdb_dbi_open(parent, c"blocked".as_ptr(), CREATE, &mut denied) },
        BAD_TXN
    );
    assert_eq!(unsafe { transaction::mdb_txn_commit(parent) }, BAD_TXN);
    unsafe { transaction::mdb_txn_abort(parent) }; // must preserve every ancestor
    assert!(unsafe { &*parent }.state.is_some());
    assert_eq!(value(child, lower), b"A");
    assert_eq!(unsafe { transaction::mdb_txn_commit(child) }, 0);
    unsafe { operations::mdb_dbi_close(f.env, lower) }; // clean, after the scope ends
    let y = open(parent, c"after", CREATE);
    put(parent, y, b"kept");
    assert_eq!(unsafe { transaction::mdb_txn_commit(parent) }, 0);
    let read = f.begin(ptr::null_mut(), RDONLY);
    assert_eq!(value(read, y), b"kept");
    let mut absent = 0;
    assert_eq!(
        unsafe { operations::mdb_dbi_open(read, c"x".as_ptr(), 0, &mut absent) },
        NOTFOUND
    );
    unsafe { transaction::mdb_txn_abort(read) };
}
