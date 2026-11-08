//! System SQLite only. No SQLite sources or production dependencies are bundled.
use crate::{
    Result,
    report::{self, EngineResult},
    workload::{Config, Workload},
};
use std::{
    ffi::{CStr, CString, c_char, c_int, c_void},
    marker::PhantomData,
    path::Path,
    ptr,
    time::Instant,
};

enum Sqlite {}
enum Stmt {}
#[link(name = "sqlite3")]
unsafe extern "C" {
    fn sqlite3_open_v2(
        name: *const c_char,
        db: *mut *mut Sqlite,
        flags: c_int,
        vfs: *const c_char,
    ) -> c_int;
    fn sqlite3_close_v2(db: *mut Sqlite) -> c_int;
    fn sqlite3_errmsg(db: *mut Sqlite) -> *const c_char;
    fn sqlite3_prepare_v2(
        db: *mut Sqlite,
        sql: *const c_char,
        n: c_int,
        stmt: *mut *mut Stmt,
        tail: *mut *const c_char,
    ) -> c_int;
    fn sqlite3_finalize(stmt: *mut Stmt) -> c_int;
    fn sqlite3_step(stmt: *mut Stmt) -> c_int;
    fn sqlite3_reset(stmt: *mut Stmt) -> c_int;
    fn sqlite3_clear_bindings(stmt: *mut Stmt) -> c_int;
    fn sqlite3_bind_blob(
        stmt: *mut Stmt,
        index: c_int,
        bytes: *const c_void,
        len: c_int,
        drop: Option<unsafe extern "C" fn(*mut c_void)>,
    ) -> c_int;
    fn sqlite3_column_blob(stmt: *mut Stmt, col: c_int) -> *const c_void;
    fn sqlite3_column_text(stmt: *mut Stmt, col: c_int) -> *const u8;
    fn sqlite3_column_bytes(stmt: *mut Stmt, col: c_int) -> c_int;
    fn sqlite3_libversion() -> *const c_char;
    fn sqlite3_sourceid() -> *const c_char;
}
const OK: c_int = 0;
const ROW: c_int = 100;
const DONE: c_int = 101;
struct Connection {
    raw: *mut Sqlite,
}
struct Statement<'a> {
    raw: *mut Stmt,
    db: &'a Connection,
    _not_send: PhantomData<*mut ()>,
}
impl Connection {
    fn open(path: &Path) -> Result<Self> {
        use std::os::unix::ffi::OsStrExt;
        let name = CString::new(path.as_os_str().as_bytes())?;
        let mut raw = ptr::null_mut();
        // SAFETY: nul-terminated path and initialized out pointer; default VFS.
        let rc = unsafe { sqlite3_open_v2(name.as_ptr(), &mut raw, 2 | 4, ptr::null()) };
        let db = Self { raw };
        if rc != OK {
            return Err(db.error(rc));
        }
        Ok(db)
    }
    fn error(&self, rc: c_int) -> Box<dyn std::error::Error> {
        let text = if self.raw.is_null() {
            "null SQLite handle".into()
        } else {
            // SAFETY: SQLite owns a nul-terminated error string through next call.
            unsafe { CStr::from_ptr(sqlite3_errmsg(self.raw)) }
                .to_string_lossy()
                .into_owned()
        };
        format!("SQLite error {rc}: {text}").into()
    }
    fn prepare(&self, sql: &str) -> Result<Statement<'_>> {
        let sql = CString::new(sql)?;
        let mut raw = ptr::null_mut();
        // SAFETY: live DB, nul-terminated SQL, valid out pointer. SQLite owns statement.
        let rc =
            unsafe { sqlite3_prepare_v2(self.raw, sql.as_ptr(), -1, &mut raw, ptr::null_mut()) };
        if rc != OK {
            if !raw.is_null() {
                unsafe {
                    sqlite3_finalize(raw);
                }
            }
            return Err(self.error(rc));
        }
        Ok(Statement {
            raw,
            db: self,
            _not_send: PhantomData,
        })
    }
    fn exec(&self, sql: &str) -> Result<()> {
        self.prepare(sql)?.run()
    }
    fn text(&self, sql: &str) -> Result<String> {
        let stmt = self.prepare(sql)?;
        // SAFETY: a live prepared statement; column pointer copied while ROW live.
        let rc = unsafe { sqlite3_step(stmt.raw) };
        if rc != ROW {
            return Err(self.error(rc));
        }
        let p = unsafe { sqlite3_column_text(stmt.raw, 0) };
        if p.is_null() {
            return Err("SQLite pragma returned NULL".into());
        }
        Ok(unsafe { CStr::from_ptr(p.cast()) }
            .to_string_lossy()
            .into_owned())
    }
}
impl Drop for Connection {
    fn drop(&mut self) {
        if !self.raw.is_null() {
            unsafe {
                sqlite3_close_v2(self.raw);
            }
        }
    }
}
impl Statement<'_> {
    fn finish<T>(&mut self, result: Result<T>) -> Result<T> {
        // SAFETY: all borrowed SQLITE_STATIC bindings remain alive until these
        // calls complete. Clear even after errors, before returning to caller.
        let reset = unsafe { sqlite3_reset(self.raw) };
        let clear = unsafe { sqlite3_clear_bindings(self.raw) };
        match result {
            Err(e) => Err(e),
            Ok(value) => {
                if reset != OK {
                    return Err(self.db.error(reset));
                }
                if clear != OK {
                    return Err(self.db.error(clear));
                }
                Ok(value)
            }
        }
    }
    fn run(&mut self) -> Result<()> {
        let rc = unsafe { sqlite3_step(self.raw) };
        let result = if rc == DONE {
            Ok(())
        } else {
            Err(self.db.error(rc))
        };
        self.finish(result)
    }
    fn bind(&self, index: c_int, bytes: &[u8; 8]) -> Result<()> {
        // SAFETY: callers run/reset/clear before bytes' lifetime ends. SQLite
        // borrows these eight bytes; no destructor or per-row malloc is requested.
        let rc = unsafe { sqlite3_bind_blob(self.raw, index, bytes.as_ptr().cast(), 8, None) };
        if rc != OK {
            Err(self.db.error(rc))
        } else {
            Ok(())
        }
    }
    fn put(&mut self, key: &[u8; 8], value: &[u8; 8]) -> Result<()> {
        let result = (|| {
            self.bind(1, key)?;
            self.bind(2, value)?;
            let rc = unsafe { sqlite3_step(self.raw) };
            if rc != DONE {
                return Err(self.db.error(rc));
            }
            Ok(())
        })();
        self.finish(result)
    }
    fn get(&mut self, key: &[u8; 8]) -> Result<Option<[u8; 8]>> {
        let result = (|| {
            self.bind(1, key)?;
            let rc = unsafe { sqlite3_step(self.raw) };
            if rc == DONE {
                return Ok(None);
            }
            if rc != ROW {
                return Err(self.db.error(rc));
            }
            let len = unsafe { sqlite3_column_bytes(self.raw, 0) };
            if len != 8 {
                return Err("SQLite value is not eight bytes".into());
            }
            let p = unsafe { sqlite3_column_blob(self.raw, 0) };
            if p.is_null() {
                return Err("SQLite returned null BLOB".into());
            }
            let mut bytes = [0; 8];
            // SAFETY: ROW owns a verified 8-byte BLOB; copy before reset/clear.
            unsafe {
                ptr::copy_nonoverlapping(p.cast::<u8>(), bytes.as_mut_ptr(), 8);
            }
            Ok(Some(bytes))
        })();
        self.finish(result)
    }
}
impl Drop for Statement<'_> {
    fn drop(&mut self) {
        unsafe {
            sqlite3_finalize(self.raw);
        }
    }
}

pub fn run(dir: &Path, c: &Config, w: &Workload) -> Result<EngineResult> {
    let setup = Instant::now();
    std::fs::create_dir(dir)?;
    let path = dir.join("data.sqlite3");
    let db = Connection::open(&path)?;
    let journal = db.text("PRAGMA journal_mode=WAL")?;
    db.exec("PRAGMA synchronous=FULL")?;
    let assigned_checkpoint = db.text("PRAGMA wal_autocheckpoint=1000")?;
    if assigned_checkpoint != "1000" {
        return Err("SQLite checkpoint assignment mismatch".into());
    }
    let synchronous = db.text("PRAGMA synchronous")?;
    let autocheckpoint = db.text("PRAGMA wal_autocheckpoint")?;
    if journal != "wal" || synchronous != "2" || autocheckpoint != "1000" {
        return Err("SQLite durable PRAGMA readback mismatch".into());
    }
    db.exec("CREATE TABLE kv(k BLOB PRIMARY KEY NOT NULL,v BLOB NOT NULL) WITHOUT ROWID")?;
    let settings = serde_json::json!({"journal_mode":journal,"synchronous":"FULL","synchronous_numeric":synchronous,"wal_autocheckpoint":autocheckpoint,"page_size":db.text("PRAGMA page_size")?,"cache_size":db.text("PRAGMA cache_size")?,"mmap_size":db.text("PRAGMA mmap_size")?,"schema":"BLOB PRIMARY KEY WITHOUT ROWID","read_api":"prepared native SELECT","open_flags":"READWRITE|CREATE"});
    let version = unsafe { CStr::from_ptr(sqlite3_libversion()) }
        .to_string_lossy()
        .into_owned();
    let source = unsafe { CStr::from_ptr(sqlite3_sourceid()) }
        .to_string_lossy()
        .into_owned();
    let setup_ns = report::nanos(setup);
    let rows = {
        let mut begin = db.prepare("BEGIN IMMEDIATE")?;
        let mut commit = db.prepare("COMMIT")?;
        let mut put = db.prepare(
            "INSERT INTO kv(k,v) VALUES(?1,?2) ON CONFLICT(k) DO UPDATE SET v=excluded.v",
        )?;
        let load = report::writes("sqlite", "load", &w.load, c, |records| {
            begin.run()?;
            for r in records {
                put.put(&r.key, &r.value)?;
            }
            commit.run()?;
            Ok(())
        })?;
        let read = {
            db.exec("BEGIN")?;
            let mut get = db.prepare("SELECT v FROM kv WHERE k=?1")?;
            report::validate(w, false, |key| get.get(key))?;
            let read = report::reads("sqlite", w, |key| get.get(key))?;
            drop(get);
            db.exec("COMMIT")?;
            read
        };
        let update = report::writes("sqlite", "update", &w.updates, c, |records| {
            begin.run()?;
            for r in records {
                put.put(&r.key, &r.value)?;
            }
            commit.run()?;
            Ok(())
        })?;
        {
            let mut get = db.prepare("SELECT v FROM kv WHERE k=?1")?;
            db.exec("BEGIN")?;
            report::validate(w, true, |key| get.get(key))?;
            drop(get);
            db.exec("COMMIT")?;
        }
        let count = db.text("SELECT count(*) FROM kv")?.parse::<usize>()?;
        if count != c.keys {
            return Err("SQLite row count oracle mismatch".into());
        }
        vec![load, read, update]
    };
    let storage_before_close = report::storage(dir)?;
    let reopen = Instant::now();
    drop(db);
    let db = Connection::open(&path)?;
    let close_reopen_ns = report::nanos(reopen);
    {
        let mut get = db.prepare("SELECT v FROM kv WHERE k=?1")?;
        db.exec("BEGIN")?;
        report::validate(w, true, |key| get.get(key))?;
        drop(get);
        db.exec("COMMIT")?;
    }
    if db.text("SELECT count(*) FROM kv")?.parse::<usize>()? != c.keys {
        return Err("reopened SQLite row count oracle mismatch".into());
    }
    let library = std::path::Path::new(env!("BENCH_SQLITE_LINK_PATH"));
    let library_hash = crate::sha_file(library)?;
    Ok(EngineResult {
        backend: "system SQLite native FFI, WAL FULL",
        versions: serde_json::json!({"sqlite":version,"source_id":source,"link_library_path":library,"link_library_sha256":library_hash}),
        settings,
        rows,
        setup_ns,
        close_reopen_ns,
        maintenance: serde_json::json!({"automatic_checkpoints_charged_to_writes":true,"explicit_maintenance":false,"last_connection_close_may_checkpoint":"included in close_reopen_ns"}),
        engine_stats: serde_json::json!({}),
        validation: report::validation(),
        storage_before_close,
    })
}
