//! Explicit adapter role selection, not cross-format LMDB file compatibility.
//! Every case runs in a fresh process, so environment variables never race.
use super::*;
use std::ffi::CString;
use std::os::unix::process::CommandExt;
use std::process::{Command, Stdio};
use std::time::{Duration, Instant};

#[test]
fn worker() {
    let Ok(role) = std::env::var("ULMDB_ALIAS_PROFILE_EXPECT") else {
        return;
    };
    let path = std::path::PathBuf::from(std::env::var_os("ULMDB_ALIAS_PROFILE_DIRECTORY").unwrap());
    let marker = std::env::var("ULMDB_ALIAS_PROFILE_MARKER").ok();
    // Native genesis is independent of the C role. This is deliberately not
    // an LMDB .9/1.0 data-file interchange or automatic detection assertion.
    drop(Environment::create(&path).unwrap());
    if let Some(marker) = marker {
        std::fs::write(path.join("LMDB-ABI"), marker).unwrap();
    }
    let name = CString::new(path.to_str().unwrap()).unwrap();
    let mut env = ptr::null_mut();
    let mut txn = ptr::null_mut();
    let mut dbi = 0;
    unsafe {
        assert_eq!(environment::mdb_env_create(&mut env), 0);
        assert_eq!(environment::mdb_env_set_maxdbs(env, 4), 0);
        assert_eq!(environment::mdb_env_open(env, name.as_ptr(), 0, 0o600), 0);
        // A live environment's catalog layout/patch role is frozen. Later
        // changes to the advisory open marker cannot retarget its serving view.
        std::fs::write(
            path.join("LMDB-ABI"),
            if role == "modern" { "0.9.31" } else { "1.0" },
        )
        .unwrap();
        assert_eq!(
            transaction::mdb_txn_begin(env, ptr::null_mut(), 0, &mut txn),
            0
        );
        assert_eq!(
            operations::mdb_dbi_open(txn, c"named".as_ptr(), CREATE, &mut dbi),
            0
        );
        let mut k = MDB_val {
            mv_size: 1,
            mv_data: b"k".as_ptr().cast_mut().cast(),
        };
        let mut v = MDB_val {
            mv_size: 1,
            mv_data: b"A".as_ptr().cast_mut().cast(),
        };
        assert_eq!(operations::mdb_put(txn, dbi, &mut k, &mut v, 0), 0);
        assert_eq!(transaction::mdb_txn_commit(txn), 0);
        assert_eq!(
            transaction::mdb_txn_begin(env, ptr::null_mut(), 0, &mut txn),
            0
        );
        let mut main = 0;
        assert_eq!(operations::mdb_dbi_open(txn, ptr::null(), 0, &mut main), 0);
        let mut cursor = ptr::null_mut();
        assert_eq!(cursor::mdb_cursor_open(txn, main, &mut cursor), 0);
        let mut ck = MDB_val {
            mv_size: 0,
            mv_data: ptr::null_mut(),
        };
        let mut cv = ck;
        assert_eq!(cursor::mdb_cursor_get(cursor, &mut ck, &mut cv, 0), 0);
        let bytes = std::slice::from_raw_parts(ck.mv_data.cast::<u8>(), ck.mv_size);
        assert_eq!(
            bytes,
            if role == "modern" {
                b"named\0".as_slice()
            } else {
                b"named".as_slice()
            }
        );
        cursor::mdb_cursor_close(cursor);
        assert_eq!(
            operations::mdb_drop(txn, main, 0),
            if role == "modern" { -30773 } else { 0 }
        );
        if role != "modern" {
            let mut anchor = MDB_val {
                mv_size: 6,
                mv_data: b"anchor".as_ptr().cast_mut().cast(),
            };
            assert_eq!(operations::mdb_put(txn, main, &mut anchor, &mut v, 0), 0);
        }
        let rc = operations::mdb_get(txn, dbi, &mut k, &mut v);
        assert_eq!(
            rc,
            match role.as_str() {
                "modern" => 0,
                "legacy36" => BAD_DBI,
                "legacy31" => NOTFOUND,
                _ => panic!("unknown role"),
            }
        );
        transaction::mdb_txn_abort(txn);
        environment::mdb_env_close(env);
    }
    std::fs::remove_dir_all(path).unwrap();
}

#[test]
fn selected_patch_and_persisted_major_are_frozen_per_open() {
    let cases = [
        (None, None, "legacy31"),
        (None, Some("0.9"), "legacy31"),
        (None, Some("0.9.31"), "legacy31"),
        (None, Some("0.9.36"), "legacy36"),
        (None, Some("1.0"), "modern"),
        (Some("0.9"), Some("0.9.36"), "legacy36"),
        (Some("0.9"), Some("1.0"), "legacy31"),
        (Some("0.9.31"), Some("0.9.36"), "legacy31"),
        (Some("0.9.31"), Some("1.0"), "legacy31"),
        (Some("0.9.36"), Some("0.9.31"), "legacy36"),
        (Some("0.9.36"), Some("1.0"), "legacy36"),
        (Some("1.0"), Some("0.9.31"), "modern"),
        (Some("1.0"), Some("0.9.36"), "modern"),
        (Some("1.0.2"), None, "modern"),
    ];
    for (index, (marker, request, expected)) in cases.into_iter().enumerate() {
        let path = std::env::temp_dir().join(format!(
            "ulmdb-role-{}-{index}-{}",
            std::process::id(),
            ulmdb_storage::new_identity().unwrap()
        ));
        let mut cmd = Command::new(std::env::current_exe().unwrap());
        cmd.args(["--exact", "alias_profile_tests::worker", "--nocapture"])
            .env_remove("ULMDB_LMDB_ABI")
            .env_remove("ULMDB_ALIAS_PROFILE_MARKER")
            .env("ULMDB_ALIAS_PROFILE_EXPECT", expected)
            .env("ULMDB_ALIAS_PROFILE_DIRECTORY", &path)
            .stdout(Stdio::piped())
            .stderr(Stdio::piped())
            .process_group(0);
        if let Some(marker) = marker {
            cmd.env("ULMDB_ALIAS_PROFILE_MARKER", marker);
        }
        if let Some(request) = request {
            cmd.env("ULMDB_LMDB_ABI", request);
        }
        let mut child = cmd.spawn().unwrap();
        let deadline = Instant::now() + Duration::from_secs(8);
        let mut timed_out = false;
        while child.try_wait().unwrap().is_none() {
            if Instant::now() >= deadline {
                timed_out = true;
                unsafe {
                    libc::kill(-(child.id() as libc::pid_t), libc::SIGKILL);
                }
                break;
            }
            std::thread::sleep(Duration::from_millis(5));
        }
        let out = child.wait_with_output().unwrap();
        assert!(
            !timed_out && out.status.success(),
            "case {index} marker={marker:?} request={request:?} expected={expected}: timeout={timed_out} {:?}: {} {}",
            out.status,
            String::from_utf8_lossy(&out.stdout),
            String::from_utf8_lossy(&out.stderr)
        );
    }
}
