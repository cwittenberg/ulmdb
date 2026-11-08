//! Tests own no production authority; exercise the actual Writer once admitted.
use crate::Error;
use sha2::{Digest, Sha256};
use std::{
    collections::BTreeMap,
    fs,
    path::{Path, PathBuf},
    process::{Command, Output},
    sync::OnceLock,
};

fn evidence_root() -> PathBuf {
    std::env::var_os("ULMDB_CHUNKS_TEST_EVIDENCE")
        .map(PathBuf::from)
        .unwrap_or_else(|| {
            std::env::temp_dir().join(format!("ulmdb-chunk-writer-{}", std::process::id()))
        })
}
fn case_directory(name: &str) -> PathBuf {
    static NEXT: std::sync::atomic::AtomicU64 = std::sync::atomic::AtomicU64::new(0);
    let serial = NEXT.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
    let path = evidence_root().join(format!("{name}-{serial}"));
    fs::create_dir_all(&path).unwrap();
    path
}
fn file_hashes(directory: &Path) -> BTreeMap<String, [u8; 32]> {
    fs::read_dir(directory)
        .unwrap()
        .map(|e| e.unwrap().path())
        .filter(|p| p.is_file())
        .map(|p| {
            (
                p.file_name().unwrap().to_string_lossy().into_owned(),
                Sha256::digest(fs::read(p).unwrap()).into(),
            )
        })
        .collect()
}
#[cfg(all(target_os = "linux", feature = "fault-injection"))]
fn syscall_shim() -> PathBuf {
    static LIBRARY: OnceLock<PathBuf> = OnceLock::new();
    LIBRARY
        .get_or_init(|| {
            let root = evidence_root().join("syscall-shim");
            fs::create_dir_all(&root).unwrap();
            let source = root.join("chunks_eio.c");
            let library = root.join("chunks_eio.so");
            fs::write(&source, SYSCALL_SHIM).unwrap();
            let mut c = Command::new("cc");
            c.args(["-shared", "-fPIC", "-O2", "-Wall", "-Wextra", "-Werror"]);
            if std::mem::size_of::<usize>() == 4 {
                c.arg("-m32");
            }
            let out = c
                .arg(source)
                .arg("-ldl")
                .arg("-o")
                .arg(&library)
                .output()
                .unwrap();
            fs::write(root.join("stdout.log"), &out.stdout).unwrap();
            fs::write(root.join("stderr.log"), &out.stderr).unwrap();
            assert!(
                out.status.success(),
                "syscall shim build: {}",
                String::from_utf8_lossy(&out.stderr)
            );
            library
        })
        .clone()
}
fn spawn_worker(
    root: &Path,
    action: &str,
    hook: Option<(&str, &str)>,
    eio: Option<&str>,
) -> Output {
    let mut cmd = Command::new(std::env::current_exe().unwrap());
    cmd.args([
        "--exact",
        "range::chunks::writer_tests::writer_worker",
        "--nocapture",
    ])
    .env("CHUNKS_WORKER_ROOT", root)
    .env("CHUNKS_WORKER_ACTION", action)
    .env("ULMDB_PACKED_TRACE_FILE", root.join("hooks.log"));
    if let Some((kind, point)) = hook {
        cmd.env(
            if kind == "cut" {
                "ULMDB_PACKED_CRASH_AT"
            } else {
                "ULMDB_PACKED_FAIL_AT"
            },
            point,
        );
    }
    if let Some(mode) = eio {
        #[cfg(all(target_os = "linux", feature = "fault-injection"))]
        cmd.env("LD_PRELOAD", syscall_shim())
            .env("CHUNKS_EIO_MODE", mode)
            .env("CHUNKS_EIO_DIRECTORY", root.join("db"))
            .env("CHUNKS_EIO_ARM", root.join("ARM"))
            .env("CHUNKS_EIO_LOG", root.join("syscall.log"));
        #[cfg(not(all(target_os = "linux", feature = "fault-injection")))]
        panic!("actual EIO fixture requires Linux and fault-injection: {mode}");
    }
    fs::write(
        root.join("case.txt"),
        format!("action={action}\nhook={hook:?}\neio={eio:?}\ncommand={cmd:?}\n"),
    )
    .unwrap();
    let out = cmd.output().unwrap();
    fs::write(root.join("stdout.log"), &out.stdout).unwrap();
    fs::write(root.join("stderr.log"), &out.stderr).unwrap();
    out
}
#[cfg(all(target_os = "linux", feature = "fault-injection"))]
const SYSCALL_SHIM: &str = r#"
#define _GNU_SOURCE
#include <dlfcn.h>
#include <errno.h>
#include <fcntl.h>
#include <limits.h>
#include <stdio.h>
#include <stdlib.h>
#include <string.h>
#include <unistd.h>
static unsigned data_calls, head_calls, directory_calls, head_writes;
static int identify(int fd, char *path) {
 const char *dir=getenv("CHUNKS_EIO_DIRECTORY"), *arm=getenv("CHUNKS_EIO_ARM");
 if(!dir || !arm || access(arm,F_OK)) return 0;
 char proc[64]; snprintf(proc,sizeof(proc),"/proc/self/fd/%d",fd);
 ssize_t n=readlink(proc,path,PATH_MAX-1); if(n<0)return 0; path[n]=0;
 if(!strcmp(path,dir))return 3;
 size_t d=strlen(dir); if(strncmp(path,dir,d) || path[d]!='/')return 0;
 const char *base=path+d+1;
 if(!strcmp(base,"RANGE7.head"))return 2;
 if(strstr(base,".chunk") || !strncmp(base,".prepare",8))return 1;
 return 0;
}
static void record(const char *op,int fd,const char *path,unsigned ordinal,long len,long off,long result,int error) {
 const char *p=getenv("CHUNKS_EIO_LOG"); if(!p)return;
 int f=open(p,O_WRONLY|O_APPEND|O_CREAT,0600); if(f<0)return;
 dprintf(f,"%s fd=%d path=%s ordinal=%u len=%ld off=%ld result=%ld errno=%d\n",op,fd,path,ordinal,len,off,result,error);close(f);
}
int fsync(int fd) {
 int (*real)(int)=dlsym(RTLD_NEXT,"fsync"); char path[PATH_MAX];int kind=identify(fd,path);
 if(!kind)return real(fd);
 unsigned n=kind==1?++data_calls:kind==2?++head_calls:++directory_calls;
 const char *m=getenv("CHUNKS_EIO_MODE");
 int fail=m && ((kind==1 && (!strcmp(m,"data") || !strcmp(m,"data-reset")) && n==1)
  ||(kind==1 && !strcmp(m,"data-reset") && n==2) ||(kind==2 && !strcmp(m,"head") && n==1)
  ||(kind==3 && !strcmp(m,"directory") && n==1));
 if(fail){record("fsync",fd,path,n,0,0,-1,EIO);errno=EIO;return -1;}
 int out=real(fd),e=errno;record("fsync",fd,path,n,0,0,out,out?e:0);errno=e;return out;
}
ssize_t pwrite64(int fd,const void *bytes,size_t len,off64_t off) {
 ssize_t (*real)(int,const void*,size_t,off64_t)=dlsym(RTLD_NEXT,"pwrite64");
 char path[PATH_MAX];int kind=identify(fd,path);const char *m=getenv("CHUNKS_EIO_MODE");
 if(kind!=2 || !m || strcmp(m,"half-head"))return real(fd,bytes,len,off);
 unsigned n=++head_writes;
 if(n==1){ssize_t out=real(fd,bytes,len/2,off);record("pwrite64",fd,path,n,len,off,out,out<0?errno:0);return out;}
 record("pwrite64",fd,path,n,len,off,-1,EIO);errno=EIO;return -1;
}
"#;

use super::{geometry, header, protocol, recovery::Recovered, writer::Writer};
use crate::range::pack::io::{read_exact_at, write_all_at};
fn payload(r: &Recovered) -> Vec<u8> {
    let mut b = vec![0; r.selected.payload_length() as usize];
    read_exact_at(
        &r.chunks.reader().unwrap(),
        &mut b,
        r.selected.start() + protocol::BLOCK as u64,
    )
    .unwrap();
    b
}
fn database(root: &Path) -> PathBuf {
    let p = root.join("db");
    fs::create_dir(&p).unwrap();
    p
}
fn seeded(root: &Path) -> PathBuf {
    let p = database(root);
    let mut w = Writer::create(&p).unwrap();
    let t = w.prepare(b"base").unwrap();
    w.publish(t).unwrap();
    p
}
fn must_succeed(o: Output) {
    assert!(
        o.status.success(),
        "{}\n{}",
        String::from_utf8_lossy(&o.stdout),
        String::from_utf8_lossy(&o.stderr)
    );
}
#[test]
fn ordinary_writer_abort_foreign_tokens_and_selected_reader_bounds() {
    let root = case_directory("ordinary");
    let db = database(&root);
    let mut w = Writer::create(&db).unwrap();
    assert_eq!(w.directory(), db);
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        for item in fs::read_dir(&db).unwrap() {
            assert_eq!(
                item.unwrap().metadata().unwrap().permissions().mode() & 0o777,
                0o600
            );
        }
    }
    let genesis = w.selected();
    assert_eq!(genesis.ordinal(), 0);
    let before = file_hashes(&db);
    let t = w.prepare(b"aborted").unwrap();
    assert_eq!(t.parent(), Some(genesis));
    assert_eq!(w.selected(), genesis);
    assert!(matches!(w.prepare(b"second"), Err(Error::Busy)));
    w.abort(t).unwrap();
    assert_eq!(w.selected(), genesis);
    assert!(!w.is_poisoned());
    assert_eq!(file_hashes(&db), before);
    let t = w.prepare(b"first").unwrap();
    let first = t.receipt();
    assert_eq!(w.publish(t).unwrap(), first);
    drop(w);
    let (mut w, old) = Writer::open_checked(&db, |r| {
        assert_eq!(payload(r), b"first");
        Ok(())
    })
    .unwrap();
    assert_eq!(old.chunks.selected_end(), first.end());
    let old_reader = old.chunks.reader().unwrap();
    let t = w.prepare(b"second").unwrap();
    let second = t.receipt();
    assert_eq!(w.selected(), first);
    assert_eq!(old_reader.read_at(&mut [0; 1], first.end()).unwrap(), 0);
    assert_eq!(w.publish(t).unwrap(), second);
    assert_eq!(payload(&old), b"first");
    let other = case_directory("foreign");
    let otherdb = database(&other);
    let mut foreign = Writer::create(&otherdb).unwrap();
    let foreign_t = foreign.prepare(b"foreign").unwrap();
    let own = w.prepare(b"own").unwrap();
    let before = file_hashes(&db);
    assert!(matches!(w.publish(foreign_t), Err(Error::Corrupt(_))));
    assert_eq!(file_hashes(&db), before);
    w.abort(own).unwrap();
    drop(w);
    let (_, current) = Writer::open_checked(&db, |r| {
        assert_eq!(payload(r), b"second");
        Ok(())
    })
    .unwrap();
    assert_eq!(current.selected, second);
    assert_eq!(payload(&old), b"first");
    drop(foreign);
    let (_, restored) = Writer::open_checked(&otherdb, |_| Ok(())).unwrap();
    assert_eq!(restored.selected.ordinal(), 0);
}
#[test]
fn rejected_selected_graph_precedes_tail_cleanup_and_corruption_fails_closed() {
    let root = case_directory("reject-before-cleanup");
    let db = seeded(&root);
    let (mut w, _) = Writer::open_checked(&db, |_| Ok(())).unwrap();
    let selected = w.selected();
    let _t = w.prepare(b"unselected").unwrap();
    drop(w);
    let before = file_hashes(&db);
    assert!(matches!(
        Writer::open_checked(&db, |_| Err(Error::LimitExceeded)),
        Err(Error::LimitExceeded)
    ));
    assert_eq!(file_hashes(&db), before);
    let (w, r) = Writer::open_checked(&db, |_| Ok(())).unwrap();
    assert_eq!(r.selected, selected);
    assert_eq!(payload(&r), b"base");
    drop(w);
    let p = db.join(header::filename(selected.binding().stream, 0));
    let f = fs::OpenOptions::new()
        .read(true)
        .write(true)
        .open(p)
        .unwrap();
    write_all_at(
        &f,
        b"X",
        geometry::locate(selected.start()).1 + protocol::BLOCK as u64,
    )
    .unwrap();
    f.sync_all().unwrap();
    let before = file_hashes(&db);
    let called = std::cell::Cell::new(false);
    assert!(
        Writer::open_checked(&db, |_| {
            called.set(true);
            Ok(())
        })
        .is_err()
    );
    assert!(!called.get());
    assert_eq!(file_hashes(&db), before);
}
#[test]
fn writer_worker() {
    let Some(root) = std::env::var_os("CHUNKS_WORKER_ROOT").map(PathBuf::from) else {
        return;
    };
    let db = root.join("db");
    let action = std::env::var("CHUNKS_WORKER_ACTION").unwrap();
    if action == "create" {
        let r = Writer::create(&db);
        assert!(r.is_err(), "injected creation error required");
        println!("create error={:?}", r.err().unwrap());
        return;
    }
    let (mut w, _) = Writer::open_checked(&db, |_| Ok(())).unwrap();
    let old = w.selected();
    fs::write(root.join("ARM"), b"armed").unwrap();
    if action == "rollover" || action == "rollover-error" {
        let mut bytes = vec![0; geometry::MAX_PAYLOAD as usize];
        bytes[0] = 0x51;
        let last = bytes.len() - 1;
        bytes[last] = 0x73;
        let result = w.prepare(&bytes);
        if action == "rollover-error" {
            assert!(matches!(result, Err(Error::Io(_))));
            assert!(!w.is_poisoned());
            assert_eq!(w.selected(), old);
            return;
        }
        let token = result.unwrap();
        assert_eq!(token.receipt().binding().index, old.binding().index + 1);
        w.publish(token).unwrap();
        return;
    }
    match action.as_str() {
        "append" => {
            let t = w.prepare(b"new").unwrap();
            w.publish(t).unwrap();
        }
        "prepare_error" | "eio-data" => {
            assert!(matches!(w.prepare(b"new"), Err(Error::Io(_))));
            assert!(!w.is_poisoned());
            assert_eq!(w.selected(), old);
        }
        "eio-data-reset" => {
            assert!(matches!(w.prepare(b"new"), Err(Error::OutcomeUnknown)));
            assert!(w.is_poisoned());
            assert_eq!(w.selected(), old);
        }
        "publish_error" | "eio-head" | "eio-half-head" => {
            let t = w.prepare(b"new").unwrap();
            assert!(matches!(w.publish(t), Err(Error::OutcomeUnknown)));
            assert!(w.is_poisoned());
            assert_eq!(w.selected(), old);
            assert!(matches!(w.prepare(b"blocked"), Err(Error::OutcomeUnknown)));
        }
        _ => panic!("unrecognized worker {action}"),
    }
}
#[cfg(all(target_os = "linux", feature = "fault-injection"))]
#[test]
fn actual_same_chunk_has_two_fences_and_known_abort_restores_ready() {
    let root = case_directory("two-fences");
    let db = seeded(&root);
    must_succeed(spawn_worker(&root, "append", None, Some("trace")));
    let s = fs::read_to_string(root.join("syscall.log")).unwrap();
    let lines: Vec<_> = s.lines().collect();
    assert_eq!(lines.len(), 2, "{s}");
    assert!(lines[0].contains(".chunk") && lines[0].contains("result=0 errno=0"));
    assert!(lines[1].contains("RANGE7.head") && lines[1].contains("result=0 errno=0"));
    let (_, r) = Writer::open_checked(&db, |_| Ok(())).unwrap();
    assert_eq!(payload(&r), b"new");
}
#[cfg(feature = "fault-injection")]
#[test]
fn append_process_cuts_and_returned_errors_preserve_strict_selected_state() {
    let prep = [
        "before_header",
        "half_header",
        "after_header",
        "half_payload",
        "after_payload",
        "before_footer",
        "half_footer",
        "after_footer",
        "before_data_sync",
        "after_data_sync",
    ];
    let publish = [
        "before_head",
        "half_head",
        "after_head",
        "before_control_sync",
        "after_control_sync",
        "before_ack",
    ];
    for (phase, names) in [("prepare", &prep[..]), ("publish", &publish[..])] {
        for name in names {
            for kind in ["cut", "error"] {
                let root = case_directory(&format!("{phase}-{name}-{kind}"));
                let db = seeded(&root);
                let point = format!("chunks.{phase}.{name}");
                let action = if kind == "cut" {
                    "append"
                } else if phase == "prepare" {
                    "prepare_error"
                } else {
                    "publish_error"
                };
                let out = spawn_worker(&root, action, Some((kind, &point)), None);
                if kind == "cut" {
                    assert_eq!(out.status.code(), Some(86), "{point}")
                } else {
                    must_succeed(out)
                }
                let (_, r) = Writer::open_checked(&db, |_| Ok(())).unwrap();
                let expected = if phase == "publish" && *name != "before_head" {
                    &b"new"[..]
                } else {
                    &b"base"[..]
                };
                assert_eq!(payload(&r), expected, "{kind}/{point}");
            }
        }
    }
}
#[cfg(feature = "fault-injection")]
#[test]
fn creation_process_cuts_never_return_ready_with_invalid_selected_bytes() {
    for name in [
        "before_chunk_header",
        "after_chunk_header",
        "half_header",
        "half_footer",
        "after_footer",
        "after_data_sync",
        "half_head0",
        "half_head1",
        "after_control_sync",
        "before_ack",
    ] {
        let root = case_directory(&format!("create-{name}"));
        let db = database(&root);
        let point = format!("chunks.create.{name}");
        let out = spawn_worker(&root, "create", Some(("cut", &point)), None);
        assert_eq!(out.status.code(), Some(86), "{point}");
        let before = file_hashes(&db);
        match Writer::open_checked(&db, |r| {
            assert_eq!(r.selected.ordinal(), 0);
            assert!(payload(r).is_empty());
            Ok(())
        }) {
            Ok((_, r)) => assert_eq!(r.selected.ordinal(), 0),
            Err(_) => assert_eq!(file_hashes(&db), before, "invalid creation must not mutate"),
        }
    }
}
#[cfg(all(target_os = "linux", feature = "fault-injection"))]
#[test]
fn actual_fsync_and_partial_head_pwrite_eio_restore_or_poison_as_required() {
    for mode in ["data", "data-reset", "head", "half-head"] {
        let root = case_directory(&format!("eio-{mode}"));
        let db = seeded(&root);
        must_succeed(spawn_worker(
            &root,
            &format!("eio-{mode}"),
            None,
            Some(mode),
        ));
        let s = fs::read_to_string(root.join("syscall.log")).unwrap();
        let errors: Vec<_> = s.lines().filter(|s| s.contains("errno=5")).collect();
        assert_eq!(
            errors.len(),
            if mode == "data-reset" { 2 } else { 1 },
            "{s}"
        );
        assert!(
            errors
                .iter()
                .all(|s| s.contains(&format!("path={}", db.display())))
        );
        if mode == "half-head" {
            let writes: Vec<_> = s.lines().filter(|s| s.starts_with("pwrite64")).collect();
            assert_eq!(writes.len(), 2);
            assert!(
                writes[0].contains("len=2048") && writes[0].contains("result=1024 errno=0"),
                "{s}"
            );
            assert!(
                writes[0].contains("off=65536") || writes[0].contains("off=131072"),
                "{s}"
            );
            let fd = writes[0]
                .split_whitespace()
                .find(|s| s.starts_with("fd="))
                .unwrap();
            assert!(writes[1].split_whitespace().any(|s| s == fd));
        }
        let (_, r) = Writer::open_checked(&db, |_| Ok(())).unwrap();
        assert_eq!(
            payload(&r),
            if mode == "head" || mode == "half-head" {
                &b"new"[..]
            } else {
                &b"base"[..]
            }
        );
    }
}

#[test]
fn active_envelope_corruption_is_rejected_before_append() {
    let root = case_directory("active-envelope-corruption");
    let db = database(&root);
    let mut w = Writer::create(&db).unwrap();
    let selected = w.selected();
    let p = db.join(header::filename(selected.binding().stream, 0));
    let f = fs::OpenOptions::new()
        .read(true)
        .write(true)
        .open(p)
        .unwrap();
    write_all_at(&f, b"X", 64).unwrap();
    f.sync_all().unwrap();
    let before = file_hashes(&db);
    assert!(matches!(
        w.prepare(b"must not append"),
        Err(Error::Corrupt(_))
    ));
    assert!(w.is_poisoned());
    assert_eq!(w.selected(), selected);
    assert_eq!(file_hashes(&db), before);
    assert!(matches!(w.prepare(b"blocked"), Err(Error::OutcomeUnknown)));
    drop(w);
    assert!(Writer::open_checked(&db, |_| Ok(())).is_err());
    assert_eq!(file_hashes(&db), before);
}
fn stream_file_hash(path: &Path) -> [u8; 32] {
    use std::io::Read;
    let mut file = fs::File::open(path).unwrap();
    let mut h = Sha256::new();
    let mut bytes = vec![0; 1 << 20];
    loop {
        let n = file.read(&mut bytes).unwrap();
        if n == 0 {
            break;
        }
        h.update(&bytes[..n]);
    }
    h.finalize().into()
}
/// Real v7 codec frames and actual sparse zero bytes. No selected-token or
/// Writer constructor is bypassed: Writer::open_checked verifies the full chain.
fn sparse_rollover_base(directory: &Path) -> protocol::Receipt {
    fs::create_dir(directory).unwrap();
    let writer = Writer::create(directory).unwrap();
    let mut selected = writer.selected();
    let stream = writer.stream();
    drop(writer);
    let path = directory.join(header::filename(stream, 0));
    let file = fs::OpenOptions::new()
        .read(true)
        .write(true)
        .open(path)
        .unwrap();
    let mut envelope = [0; geometry::HEADER_BYTES as usize];
    read_exact_at(&file, &mut envelope, 0).unwrap();
    let envelope = header::ChunkHeader::decode(&envelope).unwrap();
    let block = vec![0; 1 << 20];
    let mut zero_hash = Sha256::new();
    for _ in 0..geometry::MAX_PAYLOAD / (block.len() as u64) {
        zero_hash.update(&block);
    }
    let zero_hash: [u8; 32] = zero_hash.finalize().into();
    let mut previous = selected;
    for _ in 0..3 {
        let plan =
            geometry::AppendPlan::after(selected.descriptor(), geometry::MAX_PAYLOAD).unwrap();
        assert!(!plan.rollover);
        let (frame, bytes) = protocol::encode_frame_header(
            stream,
            plan.start,
            plan.ordinal,
            geometry::MAX_PAYLOAD,
            Some(selected),
            envelope.binding().header_hash,
            zero_hash,
        )
        .unwrap();
        let end = geometry::HEADER_BYTES + frame.receipt.end();
        file.set_len(end).unwrap();
        write_all_at(&file, &bytes, geometry::locate(plan.start).1).unwrap();
        write_all_at(
            &file,
            &protocol::encode_frame_footer(stream, &frame),
            end - protocol::BLOCK as u64,
        )
        .unwrap();
        previous = selected;
        selected = frame.receipt;
    }
    file.sync_all().unwrap();
    let control = fs::OpenOptions::new()
        .read(true)
        .write(true)
        .open(directory.join(protocol::CONTROL_FILE))
        .unwrap();
    for (at, r) in protocol::HEAD_OFFSETS.into_iter().zip([selected, previous]) {
        write_all_at(&control, &protocol::encode_head(stream, r), at).unwrap();
    }
    control.sync_all().unwrap();
    fs::File::open(directory).unwrap().sync_all().unwrap();
    selected
}
#[cfg(target_os = "linux")]
fn sparse_copy(base: &Path, destination: &Path) {
    fs::create_dir(destination).unwrap();
    for item in fs::read_dir(base).unwrap() {
        let p = item.unwrap().path();
        let out = Command::new("cp")
            .args(["--reflink=auto", "--sparse=always"])
            .arg(&p)
            .arg(destination.join(p.file_name().unwrap()))
            .output()
            .unwrap();
        assert!(
            out.status.success(),
            "sparse copy: {}",
            String::from_utf8_lossy(&out.stderr)
        );
    }
}
#[cfg(all(target_os = "linux", feature = "fault-injection"))]
#[test]
fn real_rollover_fences_recovery_abort_and_old_reader_ownership() {
    use std::os::unix::fs::MetadataExt;
    let root = case_directory("real-rollover");
    let base = root.join("base");
    let selected = sparse_rollover_base(&base);
    let name = header::filename(selected.binding().stream, 0);
    let old_path = base.join(&name);
    let old_hash = stream_file_hash(&old_path);
    assert!(
        fs::metadata(&old_path).unwrap().blocks() * 512 < 8 * 1024 * 1024,
        "preparatory zero payloads must remain sparse"
    );
    let (writer, _) = Writer::open_checked(&base, |r| {
        assert_eq!(r.selected, selected);
        Ok(())
    })
    .unwrap();
    drop(writer);
    for (label, hook, eio) in [
        (
            "private-temp-data-synced",
            Some("chunks.prepare.after_data_sync"),
            None,
        ),
        (
            "renamed-before-directory-sync",
            Some("chunks.prepare.after_chunk_rename"),
            None,
        ),
        (
            "torn-published-head",
            Some("chunks.publish.half_head"),
            None,
        ),
        ("directory-eio-known-reset", None, Some("directory")),
    ] {
        let case = case_directory(label);
        let db = case.join("db");
        sparse_copy(&base, &db);
        let out = spawn_worker(
            &case,
            if eio.is_some() {
                "rollover-error"
            } else {
                "rollover"
            },
            hook.map(|h| ("cut", h)),
            eio,
        );
        if hook.is_some() {
            assert_eq!(out.status.code(), Some(86), "{label}");
        } else {
            must_succeed(out);
        }
        let final_chunk = db.join(header::filename(selected.binding().stream, 1));
        if label == "private-temp-data-synced" {
            assert!(
                !final_chunk.exists(),
                "unsynced/private data must not own its final name"
            );
            assert!(fs::read_dir(&db).unwrap().any(|e| {
                e.unwrap()
                    .file_name()
                    .to_string_lossy()
                    .ends_with("-chunk-1.tmp")
            }));
        }
        if label == "renamed-before-directory-sync" || label == "torn-published-head" {
            assert!(
                final_chunk.exists(),
                "complete prepared final chunk expected before recovery"
            );
        }
        let (w, r) = Writer::open_checked(&db, |_| Ok(())).unwrap();
        assert_eq!(
            r.selected.binding().index,
            if label == "torn-published-head" { 1 } else { 0 }
        );
        assert_eq!(
            stream_file_hash(&db.join(&name)),
            old_hash,
            "all old bytes must remain unchanged"
        );
        if label == "torn-published-head" {
            assert_eq!(r.selected.start(), geometry::DATA_BYTES);
        }
        if eio.is_some() {
            let trace = fs::read_to_string(case.join("syscall.log")).unwrap();
            assert_eq!(trace.lines().filter(|s| s.contains("errno=5")).count(), 1);
            assert!(
                trace.lines().any(
                    |s| s.contains(&format!("path={} ", db.display())) && s.contains("errno=5")
                )
            );
        }
        drop((w, r));
        fs::remove_dir_all(&db).unwrap();
    }
    let (mut writer, old) = Writer::open_checked(&base, |_| Ok(())).unwrap();
    let old_read = old.chunks.reader().unwrap();
    let mut bytes = vec![0; geometry::MAX_PAYLOAD as usize];
    bytes[0] = 0x51;
    let last = bytes.len() - 1;
    bytes[last] = 0x73;
    let token = writer.prepare(&bytes).unwrap();
    assert_eq!(token.receipt().start(), geometry::DATA_BYTES);
    assert_eq!(writer.selected(), selected);
    writer.abort(token).unwrap();
    assert_eq!(writer.selected(), selected);
    assert!(!writer.is_poisoned());
    assert!(
        !base
            .join(header::filename(selected.binding().stream, 1))
            .exists()
    );
    assert_eq!(stream_file_hash(&old_path), old_hash);
    drop(bytes);
    // The writer owner is released while its independent old read FDs remain.
    // A child performs the successful real rollover under syscall tracing.
    drop(writer);
    let db = root.join("db");
    fs::rename(&base, &db).unwrap();
    must_succeed(spawn_worker(&root, "rollover", None, Some("trace")));
    let trace = fs::read_to_string(root.join("syscall.log")).unwrap();
    let fences: Vec<_> = trace.lines().collect();
    assert_eq!(
        fences.len(),
        3,
        "rollover requires data, directory, then selector fences: {trace}"
    );
    assert!(fences[0].contains(".prepare") && fences[0].contains("result=0 errno=0"));
    assert!(
        fences[1].contains(&format!("path={} ", db.display()))
            && fences[1].contains("result=0 errno=0")
    );
    assert!(fences[2].contains("RANGE7.head") && fences[2].contains("result=0 errno=0"));
    let hooks = fs::read_to_string(root.join("hooks.log")).unwrap();
    let mut last = 0;
    for event in [
        "chunks.prepare.after_data_sync",
        "chunks.prepare.before_chunk_rename",
        "chunks.prepare.after_chunk_rename",
        "chunks.prepare.before_directory_sync",
        "chunks.prepare.after_directory_sync",
        "chunks.publish.before_head",
        "chunks.publish.after_head",
        "chunks.publish.before_control_sync",
        "chunks.publish.after_control_sync",
        "chunks.publish.before_ack",
    ] {
        let at = hooks[last..].find(event).unwrap() + last;
        last = at + event.len();
    }
    let (w, r) = Writer::open_checked(&db, |_| Ok(())).unwrap();
    let next = r.selected;
    assert_eq!(next.binding().index, 1);
    assert_eq!(next.start(), geometry::DATA_BYTES);
    assert_eq!(
        old_read.read_at(&mut [0; 1], geometry::DATA_BYTES).unwrap(),
        0
    );
    let mut first = [9];
    read_exact_at(
        &old_read,
        &mut first,
        selected.start() + protocol::BLOCK as u64,
    )
    .unwrap();
    assert_eq!(first, [0]);
    assert_eq!(stream_file_hash(&db.join(&name)), old_hash);
    let reader = r.chunks.reader().unwrap();
    read_exact_at(&reader, &mut first, next.start() + protocol::BLOCK as u64).unwrap();
    assert_eq!(first, [0x51]);
    read_exact_at(
        &reader,
        &mut first,
        next.start() + protocol::BLOCK as u64 + geometry::MAX_PAYLOAD - 1,
    )
    .unwrap();
    assert_eq!(first, [0x73]);
    let physical = fs::metadata(db.join(header::filename(next.binding().stream, 1)))
        .unwrap()
        .len();
    assert!(physical <= geometry::HEADER_BYTES + geometry::DATA_BYTES);
    assert!(physical < next.start());
    fs::write(root.join("RESULT.txt"),format!("selected-old={selected:?}\nselected-new={next:?}\nold-file-sha256={old_hash:02x?}\nnew-physical-bytes={physical}\n")).unwrap();
    drop((w, r, reader, old, old_read));
    fs::remove_dir_all(db).unwrap();
}
