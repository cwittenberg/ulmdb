//! Isolated LMDB comparison adapter. It is not linked into ULMDB production crates.
//!
//! A mapped value cannot escape its transaction:
//! ```compile_fail,E0597
//! use ulmdb_lmdb_baseline::{Environment,Database};
//! fn cannot_escape<'env>(env:&'env Environment,db:&Database<'env>) {
//!     let value = {
//!         let read=env.begin_read().unwrap();
//!         read.get(db,b"key").unwrap().unwrap()
//!     };
//!     std::hint::black_box(value);
//! }
//! ```
//! A writer cannot invalidate a still-borrowed value:
//! ```compile_fail,E0502
//! use ulmdb_lmdb_baseline::{Environment,Database};
//! fn cannot_invalidate<'env>(env:&'env Environment,db:&Database<'env>) {
//!     let mut write=env.begin_write().unwrap();
//!     let value=write.get(db,b"key").unwrap().unwrap();
//!     write.put(db,b"key",b"replacement").unwrap();
//!     std::hint::black_box(value);
//! }
//! ```
//! A cursor cannot advance while its item is borrowed:
//! ```compile_fail,E0499
//! use ulmdb_lmdb_baseline::{Environment,Database};
//! fn cannot_advance<'env>(env:&'env Environment,db:&Database<'env>) {
//!     let read=env.begin_read().unwrap();let mut cursor=read.cursor(db).unwrap();
//!     let item=cursor.first().unwrap();
//!     cursor.next_pair().unwrap();
//!     std::hint::black_box(item);
//! }
//! ```
//! Native thread-affine handles cannot be shared across threads:
//! ```compile_fail,E0277
//! use ulmdb_lmdb_baseline::Environment;
//! fn cannot_share(env:&Environment) {
//!     std::thread::scope(|scope| {scope.spawn(|| {std::hint::black_box(env.max_key_size());});});
//! }
//! ```
mod cursor;
mod environment;
mod ffi;
mod thread_slot;
mod transaction;

pub use cursor::Cursor;
pub use environment::Environment;
pub use transaction::{Database, DatabaseKind, ReadDatabase, ReadTransaction, WriteTransaction};

use std::ffi::CStr;

pub type Result<T> = std::result::Result<T, Error>;

#[derive(Debug)]
pub enum Error {
    Lmdb(i32),
    Io(std::io::Error),
    InvalidInput(&'static str),
    AlreadyOpen,
    TransactionConflict,
    StaleDatabase,
    WrongKind,
    ExhaustedIdentity,
}
impl From<std::io::Error> for Error {
    fn from(value: std::io::Error) -> Self {
        Self::Io(value)
    }
}
impl std::fmt::Display for Error {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Lmdb(code) => {
                // SAFETY: the pinned library returns a static nul-terminated error string.
                let text = unsafe { CStr::from_ptr(ffi::mdb_strerror(*code)) }.to_string_lossy();
                write!(f, "LMDB error {code}: {text}")
            }
            Self::Io(e) => write!(f, "{e}"),
            Self::InvalidInput(s) => write!(f, "{s}"),
            Self::AlreadyOpen => write!(f, "environment already open in this process"),
            Self::TransactionConflict => write!(
                f,
                "a default transaction already occupies this thread; nested writers are unsupported"
            ),
            Self::StaleDatabase => write!(f, "foreign or uncommitted/aborted database handle"),
            Self::WrongKind => write!(f, "operation requires another database kind"),
            Self::ExhaustedIdentity => write!(f, "transaction identity space exhausted"),
        }
    }
}
impl std::error::Error for Error {}

pub(crate) fn check(code: i32) -> Result<()> {
    if code == 0 {
        Ok(())
    } else {
        Err(Error::Lmdb(code))
    }
}
pub const BUILD_INFO: &str = include_str!(env!("ULMDB_LMDB_BUILD_INFO"));

pub fn version() -> String {
    // SAFETY: null output parameters are permitted and the result is a static C string.
    unsafe {
        CStr::from_ptr(ffi::mdb_version(
            std::ptr::null_mut(),
            std::ptr::null_mut(),
            std::ptr::null_mut(),
        ))
    }
    .to_string_lossy()
    .into_owned()
}
