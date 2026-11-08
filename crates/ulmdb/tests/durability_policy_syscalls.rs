#![cfg(target_os = "linux")]
//! Explicit native integration evidence, not a power-loss or timing oracle.
//! Run with a fresh ULMDB_NATIVE_POLICY_EVIDENCE path and --ignored --exact
//! actual_native_policy_syscalls. All traces, workers and failures are retained.
use sha2::{Digest, Sha256};
use std::{
    collections::BTreeSet,
    fs,
    io::Write,
    os::unix::process::CommandExt,
    path::{Path, PathBuf},
    process::{Child, Command, ExitStatus, Stdio},
    thread,
    time::{Duration, Instant},
};
use ulmdb::{DatabaseKind, Durability, Environment, PublicationMode};

fn policy(text: &str) -> Durability {
    match text {
        "full" => Durability::Full,
        "meta" => Durability::NoMetaSync,
        "none" => Durability::NoSync,
        _ => panic!("unknown policy {text}"),
    }
}
fn mode(format: u16) -> PublicationMode {
    match format {
        2 => PublicationMode::Legacy,
        3 => PublicationMode::Cohort,
        4 => PublicationMode::Indexed,
        _ => panic!("unknown format"),
    }
}
fn mark(name: &str) {
    let text = format!("NATIVE_POLICY_PHASE:{name}\n");
    std::io::stderr().write_all(text.as_bytes()).unwrap();
    std::io::stderr().flush().unwrap();
}
fn selected(path: &Path) -> ulmdb_storage::FileDurabilityState {
    let (_, capture, pin, state) =
        ulmdb_storage::coordination::Coordination::capture_directory(path, Default::default())
            .unwrap()
            .capture_current_with_durability()
            .unwrap();
    assert!(state.matches(pin.root().checkpoint()));
    drop(pin);
    drop(capture);
    state
}
fn value(read: &ulmdb::ReadTxn, expected: &[u8]) {
    assert_eq!(read.get(b"data", b"key").unwrap(), Some(expected));
}

fn asset_inventory(directory: &Path, name: &str) {
    let (current, previous) = ulmdb_storage::read_roots(directory).unwrap();
    let mut assets = BTreeSet::new();
    for manifest in std::iter::once(&current).chain(previous.as_ref()) {
        assets.insert(ulmdb_storage::descriptor_path(directory, manifest));
        for entry in manifest.segments.iter() {
            assets.insert(ulmdb_storage::segment_path(directory, entry));
        }
    }
    let text = assets
        .iter()
        .map(|p| format!("{}\n", p.display()))
        .collect::<String>();
    fs::write(directory.parent().unwrap().join(name), text).unwrap();
}

#[test]
fn native_policy_worker() {
    let Some(directory) = std::env::var_os("ULMDB_NATIVE_POLICY_WORK") else {
        return;
    };
    let directory = PathBuf::from(directory);
    let format: u16 = std::env::var("ULMDB_NATIVE_POLICY_FORMAT")
        .unwrap()
        .parse()
        .unwrap();
    let role = std::env::var("ULMDB_NATIVE_POLICY_ROLE").unwrap();
    if role == "timeout" {
        mark("timeout.ready");
        thread::sleep(Duration::from_secs(30));
        panic!("owned timeout worker unexpectedly survived");
    }
    let env = Environment::create_with_publication(&directory, mode(format)).unwrap();
    let mut seed = env.begin_write().unwrap();
    seed.create_database(b"data", DatabaseKind::Kv).unwrap();
    seed.put(b"data", b"key", b"A").unwrap();
    seed.commit().unwrap();
    let old_a = env.begin_read();
    let a_sequence = old_a.sequence();
    value(&old_a, b"A");
    let metadata = policy(&std::env::var("ULMDB_NATIVE_POLICY_METADATA").unwrap());
    if role == "rollback" {
        env.set_durability(Durability::NoSync).unwrap();
        let mut writer = env.begin_write().unwrap();
        writer.put(b"data", b"key", b"B").unwrap();
        let prepared = writer.prepare().unwrap();
        assert_eq!(prepared.data_policy(), Durability::NoSync);
        prepared.commit().unwrap();
        let old_b = env.begin_read();
        value(&old_b, b"B");
        assert!(selected(&directory).has_data_debt());
        asset_inventory(&directory, "rollback-inherited-assets.txt");
        env.set_durability(metadata).unwrap();
        mark("rollback.begin");
        let report = env
            .rollback_last_commit_with_metadata_policy(old_b.sequence(), metadata)
            .unwrap();
        mark("rollback.end");
        assert_eq!(report.sequence, a_sequence);
        fs::write(
            directory
                .parent()
                .unwrap()
                .join("rollback-new-descriptor.txt"),
            ulmdb_storage::descriptor_path(&directory, env.begin_read().file_manifest().unwrap())
                .to_str()
                .unwrap(),
        )
        .unwrap();
        assert_eq!(env.begin_read().sequence(), a_sequence);
        value(&env.begin_read(), b"A");
        value(&old_b, b"B");
        value(&old_a, b"A");
        let restored = selected(&directory);
        assert!(
            restored.has_data_debt(),
            "rollback must preserve inherited debt"
        );
        assert!(restored.is_weak());
        assert!(ulmdb_storage::read_roots(&directory).unwrap().1.is_none());
        drop(env);
        let reopened = Environment::open(&directory).unwrap();
        value(&reopened.begin_read(), b"A");
        assert_eq!(selected(&directory), restored);
        println!("NATIVE_POLICY_PASS role=rollback format={format}");
        return;
    }
    let data = policy(&std::env::var("ULMDB_NATIVE_POLICY_DATA").unwrap());
    let local = policy(&std::env::var("ULMDB_NATIVE_POLICY_LOCAL").unwrap());
    env.set_durability(data).unwrap();
    let mut writer = env.begin_write_with_durability(local).unwrap();
    writer.put(b"data", b"key", b"B").unwrap();
    mark("prepare.begin");
    let prepared = writer.prepare().unwrap();
    mark("prepare.end");
    assert_eq!(prepared.data_policy(), data.weaken(local));
    value(&env.begin_read(), b"A");
    value(&old_a, b"A");
    env.set_durability(metadata).unwrap();
    mark("final.begin");
    let b_sequence = prepared.commit().unwrap();
    mark("final.end");
    assert_eq!(b_sequence, a_sequence + 1);
    value(&env.begin_read(), b"B");
    value(&old_a, b"A");
    let before = selected(&directory);
    assert_eq!(
        before.has_data_debt(),
        data.weaken(local) == Durability::NoSync
    );
    assert_eq!(
        before.is_weak(),
        metadata.weaken(local) != Durability::Full || before.has_data_debt()
    );
    let current_bytes = fs::read(directory.join("CURRENT")).unwrap();
    asset_inventory(&directory, "force-assets.txt");
    env.set_durability(Durability::NoSync).unwrap();
    mark("skip.begin");
    assert!(env.sync_with_force(false).unwrap().is_none());
    mark("skip.end");
    mark("force.begin");
    let report = env.sync_with_force(true).unwrap().unwrap();
    mark("force.end");
    assert!(report.files_synced > 0 && report.directories_synced > 0);
    assert_eq!(fs::read(directory.join("CURRENT")).unwrap(), current_bytes);
    assert_eq!(
        selected(&directory),
        before,
        "sync must not rewrite debt metadata"
    );
    value(&old_a, b"A");
    drop(env);
    let reopened = Environment::open(&directory).unwrap();
    value(&reopened.begin_read(), b"B");
    assert_eq!(selected(&directory), before);
    println!("NATIVE_POLICY_PASS role=commit format={format}");
}

struct OwnedGroup(Option<Child>);
impl OwnedGroup {
    fn wait(&mut self, deadline: Duration) -> (ExitStatus, bool) {
        let start = Instant::now();
        loop {
            let child = self.0.as_mut().unwrap();
            if let Some(status) = child.try_wait().unwrap() {
                self.0.take();
                return (status, false);
            }
            if start.elapsed() >= deadline {
                let id = child.id();
                assert!(id > 1 && id <= i32::MAX as u32);
                // process_group(0) makes this spawned tracer the owned group
                // leader. Never signal an inherited or caller-selected PGID.
                let _ = Command::new("kill")
                    .args(["-KILL", "--", &format!("-{id}")])
                    .status()
                    .expect("signal owned tracer process group");
                let status = child.wait().unwrap();
                self.0.take();
                return (status, true);
            }
            thread::sleep(Duration::from_millis(5));
        }
    }
}
impl Drop for OwnedGroup {
    fn drop(&mut self) {
        if let Some(child) = self.0.as_mut() {
            let id = child.id();
            if id > 1 && id <= i32::MAX as u32 {
                let _ = Command::new("kill")
                    .args(["-KILL", "--", &format!("-{id}")])
                    .status();
            }
            let _ = child.wait();
        }
    }
}
fn hash(path: &Path) -> String {
    format!("{:x}", Sha256::digest(fs::read(path).unwrap()))
}
fn section<'a>(log: &'a str, name: &str) -> &'a str {
    let begin = format!("NATIVE_POLICY_PHASE:{name}.begin\\n");
    let end = format!("NATIVE_POLICY_PHASE:{name}.end\\n");
    assert_eq!(
        log.matches(&begin).count(),
        1,
        "missing/duplicate {name} begin"
    );
    assert_eq!(log.matches(&end).count(), 1, "missing/duplicate {name} end");
    let start = log.find(&begin).unwrap();
    let start = start + log[start..].find('\n').unwrap() + 1;
    let finish = log.find(&end).unwrap();
    let finish = log[..finish].rfind('\n').map_or(0, |i| i + 1);
    assert!(start <= finish, "{name} marker order");
    &log[start..finish]
}
fn fence(line: &str) -> bool {
    (line.contains("fsync(") || line.contains("fdatasync(")) && line.ends_with("= 0")
}
fn data_asset(line: &str) -> bool {
    line.contains("-segment.tmp>") || line.contains(".seg>") || line.contains(".cohort>")
}
fn descriptor_asset(line: &str) -> bool {
    line.contains(".root>") || line.contains(".cohort>")
}
fn asset_open(line: &str) -> bool {
    line.contains("openat(")
        && ((line.contains("/segments/")
            && (line.contains(".seg\"") || line.contains("-segment.tmp\"")))
            || (line.contains("/cohorts/") && line.contains(".cohort\""))
            || (line.contains("/commits/") && line.contains(".root\"")))
}
fn root_fence(line: &str, database: &Path) -> bool {
    fence(line) && line.contains(&format!("<{}>", database.display()))
}
fn current_tmp_fence(line: &str) -> bool {
    fence(line) && line.contains("-CURRENT.tmp>")
}
fn check_commit(log: &str, database: &Path, format: u16, data: Durability, metadata: Durability) {
    let prepare = section(log, "prepare");
    let final_step = section(log, "final");
    assert_eq!(
        prepare.lines().any(|l| fence(l) && data_asset(l)),
        data != Durability::NoSync,
        "format{format}: data fencing differs: {prepare}"
    );
    assert_eq!(
        prepare.lines().any(|l| fence(l) && descriptor_asset(l)),
        data != Durability::NoSync,
        "format{format}: descriptor fencing differs: {prepare}"
    );
    assert!(
        !final_step.lines().any(asset_open),
        "asset/descriptor opened during final: {final_step}"
    );
    assert!(
        !final_step
            .lines()
            .any(|l| fence(l) && (data_asset(l) || descriptor_asset(l))),
        "asset/descriptor fenced during final: {final_step}"
    );
    assert_eq!(
        final_step.lines().any(current_tmp_fence),
        metadata != Durability::NoSync,
        "CURRENT fencing differs: {final_step}"
    );
    assert_eq!(
        final_step.lines().any(|l| root_fence(l, database)),
        metadata == Durability::Full,
        "root directory fencing differs: {final_step}"
    );
    let skip = section(log, "skip");
    assert!(
        !skip.lines().any(fence),
        "nonforced NoSync was not skipped: {skip}"
    );
    assert!(
        !skip
            .lines()
            .any(|l| l.contains("openat(") && l.contains("/CURRENT\"")),
        "nonforced NoSync read CURRENT: {skip}"
    );
    let force = section(log, "force");
    assert!(
        force.lines().any(|l| fence(l) && data_asset(l)),
        "force did not fence data: {force}"
    );
    assert!(
        force.lines().any(|l| fence(l) && descriptor_asset(l)),
        "force did not fence descriptors: {force}"
    );
    assert!(
        force.lines().any(|l| fence(l) && l.contains("/CURRENT>")),
        "force did not fence CURRENT: {force}"
    );
    assert!(
        force.lines().any(|l| root_fence(l, database)),
        "force did not fence root directory: {force}"
    );
    let assets = fs::read_to_string(database.parent().unwrap().join("force-assets.txt")).unwrap();
    assert!(!assets.is_empty());
    for asset in assets.lines() {
        assert!(
            force
                .lines()
                .any(|l| fence(l) && l.contains(&format!("<{asset}>"))),
            "force skipped authenticated selected/previous asset {asset}: {force}"
        );
    }
}
fn check_rollback(log: &str, database: &Path, metadata: Durability) {
    let rollback = section(log, "rollback");
    assert!(
        !rollback
            .lines()
            .any(|l| fence(l) && (l.contains(".seg>") || l.contains("-segment.tmp>"))),
        "rollback fenced user segments: {rollback}"
    );
    assert_eq!(
        rollback.lines().any(current_tmp_fence),
        metadata != Durability::NoSync,
        "rollback CURRENT fencing differs: {rollback}"
    );
    assert_eq!(
        rollback.lines().any(|l| root_fence(l, database)),
        metadata == Durability::Full,
        "rollback root directory fencing differs: {rollback}"
    );
    // Compare with independently enumerated pre-marker A/B assets. The new
    // metadata-only R cohort may be synced; inherited user assets may not.
    let inherited = fs::read_to_string(
        database
            .parent()
            .unwrap()
            .join("rollback-inherited-assets.txt"),
    )
    .unwrap();
    assert!(!inherited.is_empty());
    for old in inherited.lines() {
        assert!(
            !rollback
                .lines()
                .any(|l| fence(l) && l.contains(&format!("<{old}>"))),
            "rollback refenced inherited A/B asset {old}: {rollback}"
        );
    }
    let new_descriptor = fs::read_to_string(
        database
            .parent()
            .unwrap()
            .join("rollback-new-descriptor.txt"),
    )
    .unwrap();
    assert_eq!(
        rollback
            .lines()
            .any(|l| fence(l) && l.contains(&format!("<{new_descriptor}>"))),
        metadata != Durability::NoSync,
        "rollback R descriptor fencing differs: {rollback}"
    );
}

#[derive(Clone, Copy)]
struct Case {
    format: u16,
    data: &'static str,
    metadata: &'static str,
    local: &'static str,
    role: &'static str,
}
fn run_case(root: &Path, executable: &Path, case: Case, index: usize, summary: &mut fs::File) {
    let name = format!(
        "{index:02}-format{}-{}-{}-{}-{}",
        case.format, case.role, case.data, case.metadata, case.local
    );
    let path = root.join(&name);
    fs::create_dir(&path).unwrap();
    let database = path.join("database");
    let trace = path.join("syscalls.log");
    let mut command = Command::new("strace");
    command
        .args([
            "-f",
            "-qq",
            "-yy",
            "-s",
            "4096",
            "-e",
            "trace=openat,fsync,fdatasync,rename,renameat,renameat2,write,writev,close",
            "-o",
        ])
        .arg(&trace)
        .arg(executable)
        .args([
            "--exact",
            "native_policy_worker",
            "--nocapture",
            "--test-threads=1",
        ])
        .env("ULMDB_NATIVE_POLICY_WORK", &database)
        .env("ULMDB_NATIVE_POLICY_FORMAT", case.format.to_string())
        .env("ULMDB_NATIVE_POLICY_ROLE", case.role)
        .env("ULMDB_NATIVE_POLICY_DATA", case.data)
        .env("ULMDB_NATIVE_POLICY_METADATA", case.metadata)
        .env("ULMDB_NATIVE_POLICY_LOCAL", case.local)
        .env_remove("ULMDB_FAIL_AT")
        .env_remove("ULMDB_CRASH_AT")
        .env_remove("ULMDB_PAUSE_AT")
        .env_remove("ULMDB_TRACE_FILE")
        .stdin(Stdio::null())
        .stdout(fs::File::create(path.join("stdout.log")).unwrap())
        .stderr(fs::File::create(path.join("stderr.log")).unwrap())
        .process_group(0);
    fs::write(path.join("command.txt"), format!("{command:?}\n")).unwrap();
    let mut owned = OwnedGroup(Some(command.spawn().expect("strace is required")));
    let (status, timeout) = owned.wait(Duration::from_secs(15));
    let exit = status
        .code()
        .map_or_else(|| "null".to_string(), |code| code.to_string());
    fs::write(
        path.join("status.json"),
        format!(
            "{{\"success\":{},\"timeout\":{},\"exit\":{exit}}}\n",
            status.success(),
            timeout
        ),
    )
    .unwrap();
    assert!(
        !timeout,
        "owned worker exceeded15s; evidence {}",
        path.display()
    );
    assert!(
        status.success(),
        "native worker failed; evidence {}",
        path.display()
    );
    let log = fs::read_to_string(&trace).unwrap();
    if case.role == "rollback" {
        check_rollback(&log, &database, policy(case.metadata));
    } else {
        check_commit(
            &log,
            &database,
            case.format,
            policy(case.data).weaken(policy(case.local)),
            policy(case.metadata).weaken(policy(case.local)),
        );
    }
    writeln!(
        summary,
        "{name}\tPASS\t{}\t{}",
        hash(&trace),
        hash(&path.join("stdout.log"))
    )
    .unwrap();
    summary.flush().unwrap();
}

#[test]
#[ignore = "explicit real native syscall evidence; requires strace and a fresh retained output path"]
fn actual_native_policy_syscalls() {
    let root = PathBuf::from(
        std::env::var_os("ULMDB_NATIVE_POLICY_EVIDENCE")
            .expect("fresh retained evidence path required"),
    );
    fs::create_dir(&root).expect("evidence path must be fresh, with an existing parent");
    let root = root.canonicalize().unwrap();
    fs::write(
        root.join("test-source.rs"),
        include_bytes!("durability_policy_syscalls.rs"),
    )
    .unwrap();
    let executable = root.join("worker-executable");
    fs::copy(std::env::current_exe().unwrap(), &executable).unwrap();
    fs::write(root.join("inputs.json"), format!("{{\"scope\":\"native real syscall behavior, not powerloss or timing\",\"source_sha256\":\"{}\",\"executable_sha256\":\"{}\",\"deadline_seconds\":15}}\n", hash(&root.join("test-source.rs")), hash(&executable))).unwrap();
    fs::write(
        root.join("receipt.json"),
        "{\"complete\":false,\"planned_cases\":42}\n",
    )
    .unwrap();
    let mut summary = fs::File::create(root.join("cases.tsv")).unwrap();
    writeln!(summary, "case\tresult\ttrace_sha256\tstdout_sha256").unwrap();
    let mut count = 0;
    for format in [2, 3, 4] {
        for data in ["full", "meta", "none"] {
            for metadata in ["full", "meta", "none"] {
                run_case(
                    &root,
                    &executable,
                    Case {
                        format,
                        data,
                        metadata,
                        local: "full",
                        role: "commit",
                    },
                    count,
                    &mut summary,
                );
                count += 1;
            }
        }
        for local in ["meta", "none"] {
            run_case(
                &root,
                &executable,
                Case {
                    format,
                    data: "full",
                    metadata: "full",
                    local,
                    role: "commit",
                },
                count,
                &mut summary,
            );
            count += 1;
        }
        for metadata in ["full", "meta", "none"] {
            run_case(
                &root,
                &executable,
                Case {
                    format,
                    data: "none",
                    metadata,
                    local: "full",
                    role: "rollback",
                },
                count,
                &mut summary,
            );
            count += 1;
        }
    }
    assert_eq!(count, 42);
    assert_eq!(hash(&executable), hash(&std::env::current_exe().unwrap()));
    fs::write(root.join("receipt.json"), format!("{{\"complete\":true,\"cases\":42,\"prepare_final_pairs\":27,\"local_cases\":6,\"rollback_cases\":9,\"cases_sha256\":\"{}\"}}\n", hash(&root.join("cases.tsv")))).unwrap();
    println!(
        "NATIVE_POLICY_SYSCALLS_PASS cases=42 evidence={}",
        root.display()
    );
}
