//! Test-only policy adapter; actual production Environment methods are called.
use std::{ops::Deref, path::Path};
use ulmdb::resident::{Environment as Actual, Options, RedoEncoding, RedoFormat};
pub struct Environment(Actual);
fn encoding(f: RedoFormat) -> RedoEncoding {
    if f == RedoFormat::LiteralV1 {
        RedoEncoding::Owned
    } else {
        RedoEncoding::Borrowed
    }
}
#[allow(dead_code)] // Each copied suite exercises a different subset of policy helpers.
impl Environment {
    pub fn create_with_redo(p: impl AsRef<Path>, o: Options, f: RedoFormat) -> ulmdb::Result<Self> {
        Actual::create_with_encoding(p, o, f, encoding(f)).map(|e| {
            assert_eq!(e.redo_encoding(), encoding(f));
            Self(e)
        })
    }
    pub fn open_with_redo(p: impl AsRef<Path>, o: Options, f: RedoFormat) -> ulmdb::Result<Self> {
        Actual::open_with_encoding(p, o, f, encoding(f)).map(|e| {
            assert_eq!(e.redo_encoding(), encoding(f));
            Self(e)
        })
    }
    pub fn open(p: impl AsRef<Path>, o: Options) -> ulmdb::Result<Self> {
        let h = std::fs::read(p.as_ref().join("RESIDENT.wal"))?;
        let f = if h.starts_with(b"ULMDBRW1") {
            RedoFormat::LiteralV1
        } else {
            RedoFormat::CompactV2
        };
        Self::open_with_redo(p, o, f)
    }
}
impl Deref for Environment {
    type Target = Actual;
    fn deref(&self) -> &Actual {
        &self.0
    }
}
