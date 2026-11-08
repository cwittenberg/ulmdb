//! ABI declarations checked against pinned LMDB 1.0.2 lmdb.h, default native ABI.
use std::ffi::{c_char, c_int, c_uint, c_void};

#[repr(C)]
pub struct Env {
    _private: [u8; 0],
}
#[repr(C)]
pub struct Txn {
    _private: [u8; 0],
}
#[repr(C)]
pub struct Cursor {
    _private: [u8; 0],
}
#[repr(C)]
pub struct Val {
    pub size: usize,
    pub data: *mut c_void,
}

pub const RDONLY: c_uint = 0x20000;
pub const NOTLS: c_uint = 0x200000;
pub const CREATE: c_uint = 0x40000;
pub const DUPSORT: c_uint = 0x04;
pub const NODUPDATA: c_uint = 0x20;
pub const NOTFOUND: c_int = -30798;
pub const KEYEXIST: c_int = -30799;

pub const FIRST: c_int = 0;
pub const GET_BOTH: c_int = 2;
pub const GET_CURRENT: c_int = 4;
pub const LAST: c_int = 6;
pub const NEXT: c_int = 8;
pub const NEXT_DUP: c_int = 9;
pub const NEXT_NODUP: c_int = 11;
pub const PREV: c_int = 12;
pub const SET_RANGE: c_int = 17;

unsafe extern "C" {
    pub fn mdb_version(major: *mut c_int, minor: *mut c_int, patch: *mut c_int) -> *const c_char;
    pub fn mdb_strerror(error: c_int) -> *const c_char;
    pub fn mdb_env_create(env: *mut *mut Env) -> c_int;
    pub fn mdb_env_set_mapsize(env: *mut Env, size: usize) -> c_int;
    pub fn mdb_env_set_maxdbs(env: *mut Env, count: c_uint) -> c_int;
    pub fn mdb_env_open(env: *mut Env, path: *const c_char, flags: c_uint, mode: c_uint) -> c_int;
    pub fn mdb_env_get_maxkeysize(env: *mut Env) -> c_int;
    pub fn mdb_env_close(env: *mut Env);
    pub fn mdb_txn_begin(
        env: *mut Env,
        parent: *mut Txn,
        flags: c_uint,
        txn: *mut *mut Txn,
    ) -> c_int;
    pub fn mdb_txn_commit(txn: *mut Txn) -> c_int;
    pub fn mdb_txn_abort(txn: *mut Txn);
    pub fn mdb_dbi_open(
        txn: *mut Txn,
        name: *const c_char,
        flags: c_uint,
        db: *mut c_uint,
    ) -> c_int;
    pub fn mdb_dbi_flags(txn: *mut Txn, db: c_uint, flags: *mut c_uint) -> c_int;
    pub fn mdb_get(txn: *mut Txn, db: c_uint, key: *mut Val, value: *mut Val) -> c_int;
    pub fn mdb_put(
        txn: *mut Txn,
        db: c_uint,
        key: *mut Val,
        value: *mut Val,
        flags: c_uint,
    ) -> c_int;
    pub fn mdb_cursor_open(txn: *mut Txn, db: c_uint, cursor: *mut *mut Cursor) -> c_int;
    pub fn mdb_cursor_close(cursor: *mut Cursor);
    pub fn mdb_cursor_get(cursor: *mut Cursor, key: *mut Val, value: *mut Val, op: c_int) -> c_int;
}
