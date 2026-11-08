//! Real-process journal/projection durability journeys. An abrupt process exit
//! retains the operating-system cache; this is not a physical power-loss test.
use std::{
    path::{Path, PathBuf},
    process::{Command, Stdio},
    thread,
    time::{Duration, Instant},
};

use ulmdb::{DatabaseKind, Environment, Result, WriteTxn};
use ulmdb_ipdon::journal::{self, ApplyMode, Event, Limits};

const APPLICATION: &[u8] = b"process-test.current";

struct Directory(PathBuf);
impl Directory {
    fn new() -> Self {
        Self(std::env::temp_dir().join(format!(
            "ulmdb-ipdon-process-{:032x}",
            ulmdb_storage::new_identity().unwrap()
        )))
    }
}
impl Drop for Directory {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.0);
    }
}

fn event(sequence: u64, value: &[u8]) -> Event {
    Event {
        origin: 7,
        sequence,
        recorded_at: 1000 + sequence,
        payload: value.to_vec(),
    }
}

fn project(write: &mut WriteTxn<'_>, event: &Event) -> Result<()> {
    // Independent application records make a partially applied callback visible.
    write.put(APPLICATION, b"forward", &event.payload)?;
    write.put(APPLICATION, b"inverse", &event.payload)?;
    write.put(APPLICATION, &event.origin.to_be_bytes(), &event.payload)
}

fn initialize(path: &Path) {
    initialize_value(path, b"old");
}

fn initialize_value(path: &Path, value: &[u8]) {
    let env = Environment::create(path).unwrap();
    let mut write = env.begin_write().unwrap();
    journal::initialize_with_realm(&mut write, 0x5245_4749_4f4e_414c).unwrap();
    write
        .create_database(APPLICATION, DatabaseKind::Kv)
        .unwrap();
    journal::apply_page(
        &mut write,
        &[event(1, value)],
        Limits::default(),
        ApplyMode::Synchronous,
        project,
    )
    .unwrap();
    write.commit().unwrap();
}

#[test]
fn regional_process_worker() {
    let Some(path) = std::env::var_os("ULMDB_IPDON_PROCESS_DATABASE") else {
        return;
    };
    let point = std::env::var("ULMDB_IPDON_PROCESS_EXIT").unwrap();
    let env = Environment::open(path).unwrap();
    let mut write = env.begin_write().unwrap();
    journal::apply_page(
        &mut write,
        &[event(2, b"new")],
        Limits::default(),
        ApplyMode::Synchronous,
        project,
    )
    .unwrap();
    if point == "before_commit" {
        std::process::exit(86);
    }
    write.commit().unwrap();
    if point == "after_commit" {
        std::process::exit(86);
    }
    assert_eq!(point, "normal");
}

fn run_worker(path: &Path, point: &str) {
    let mut child = Command::new(std::env::current_exe().unwrap())
        .args(["--exact", "regional_process_worker", "--nocapture"])
        .env("ULMDB_IPDON_PROCESS_DATABASE", path)
        .env("ULMDB_IPDON_PROCESS_EXIT", point)
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .unwrap();
    let deadline = Instant::now() + Duration::from_secs(30);
    loop {
        if child.try_wait().unwrap().is_some() {
            break;
        }
        if Instant::now() >= deadline {
            child.kill().unwrap();
            let output = child.wait_with_output().unwrap();
            panic!(
                "journal worker timed out at {point}: {} {}",
                String::from_utf8_lossy(&output.stdout),
                String::from_utf8_lossy(&output.stderr)
            );
        }
        thread::sleep(Duration::from_millis(10));
    }
    let output = child.wait_with_output().unwrap();
    let expected = if point == "normal" { 0 } else { 86 };
    assert_eq!(
        output.status.code(),
        Some(expected),
        "journal worker failed at {point}: {} {}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );
}

#[test]
fn process_exit_preserves_atomic_projection_cursor_and_future_sequence() {
    for point in ["before_commit", "after_commit", "normal"] {
        let directory = Directory::new();
        initialize(&directory.0);
        run_worker(&directory.0, point);

        let env = Environment::open(&directory.0).unwrap();
        let read = env.begin_read();
        let expected_sequence = if point == "before_commit" { 1 } else { 2 };
        let expected: &[u8] = if expected_sequence == 1 {
            b"old"
        } else {
            b"new"
        };
        let cursor = journal::cursor(&read, 7).unwrap();
        assert_eq!(cursor.applied, expected_sequence, "{point}");
        assert_eq!(cursor.serving, expected_sequence, "{point}");
        assert_eq!(read.get(APPLICATION, b"forward").unwrap(), Some(expected));
        assert_eq!(read.get(APPLICATION, b"inverse").unwrap(), Some(expected));
        let page = journal::page(&read, 7, 0, Limits::default()).unwrap();
        assert_eq!(page.events.len() as u64, expected_sequence);
        assert_eq!(page.events.last().unwrap().payload, expected);

        // A failed/abandoned write must not consume the next origin identity.
        let mut write = env.begin_write().unwrap();
        journal::apply_page(
            &mut write,
            &[event(expected_sequence + 1, b"continued")],
            Limits::default(),
            ApplyMode::Synchronous,
            project,
        )
        .unwrap();
        write.commit().unwrap();
        assert_eq!(
            journal::cursor(&env.begin_read(), 7).unwrap().applied,
            expected_sequence + 1
        );
        // The retained reader still exposes its complete older generation.
        assert_eq!(read.get(APPLICATION, b"forward").unwrap(), Some(expected));
    }
}

fn append(env: &Environment, origin: u64, sequence: u64, value: &[u8]) {
    let mut write = env.begin_write().unwrap();
    let mut next = event(sequence, value);
    next.origin = origin;
    journal::apply_page(
        &mut write,
        &[next],
        Limits::default(),
        ApplyMode::Synchronous,
        project,
    )
    .unwrap();
    write.commit().unwrap();
}

#[test]
fn peer_checkpoint_rebase_preserves_local_unreplicated_tail_and_origin_identity() {
    let directory = Directory::new();
    std::fs::create_dir(&directory.0).unwrap();
    let local_path = directory.0.join("local");
    initialize(&local_path);
    let local = Environment::open(&local_path).unwrap();
    let baseline = directory.0.join("baseline");
    let base_receipt = journal::export_snapshot(&local, &local.begin_read(), &baseline).unwrap();
    assert_eq!(base_receipt.cursors()[&7].applied, 1);

    // A WAN partition now leaves each side with observations the other lacks.
    let peer = journal::restore_snapshot(&baseline, directory.0.join("peer")).unwrap();
    append(&local, 7, 2, b"local-unreplicated");
    append(&peer, 8, 1, b"peer-observation");
    let peer_checkpoint = directory.0.join("peer-checkpoint");
    let peer_receipt =
        journal::export_snapshot(&peer, &peer.begin_read(), &peer_checkpoint).unwrap();
    assert_eq!(peer_receipt.cursors()[&7].applied, 1);
    assert_eq!(peer_receipt.cursors()[&8].applied, 1);

    let rebased_path = directory.0.join("rebased");
    let rebased = journal::rebase_snapshot(
        &peer_checkpoint,
        &local,
        7,
        &rebased_path,
        Limits::default(),
        project,
    )
    .unwrap();
    let read = rebased.begin_read();
    assert_eq!(journal::cursor(&read, 7).unwrap().applied, 2);
    assert_eq!(journal::cursor(&read, 7).unwrap().serving, 2);
    assert_eq!(journal::cursor(&read, 8).unwrap().applied, 1);
    assert_eq!(
        read.get(APPLICATION, &7u64.to_be_bytes()).unwrap(),
        Some(b"local-unreplicated".as_slice())
    );
    assert_eq!(
        read.get(APPLICATION, &8u64.to_be_bytes()).unwrap(),
        Some(b"peer-observation".as_slice())
    );
    // Bootstrapping an existing node must preserve its origin sequence space.
    append(&rebased, 7, 3, b"local-continued");
    assert_eq!(
        journal::cursor(&rebased.begin_read(), 7).unwrap().applied,
        3
    );
    // Recovery is staged into a new directory. Its source remains available.
    assert_eq!(journal::cursor(&local.begin_read(), 7).unwrap().applied, 2);
    assert_eq!(journal::cursor(&peer.begin_read(), 7).unwrap().applied, 1);
    drop(read);
    drop(rebased);
    let reopened = Environment::open(&rebased_path).unwrap();
    assert_eq!(
        journal::cursor(&reopened.begin_read(), 7).unwrap().applied,
        3
    );
    assert_eq!(
        reopened
            .begin_read()
            .get(APPLICATION, &8u64.to_be_bytes())
            .unwrap(),
        Some(b"peer-observation".as_slice())
    );
}

#[test]
fn rebase_rejects_forked_origin_history_even_when_last_event_matches() {
    let directory = Directory::new();
    std::fs::create_dir(&directory.0).unwrap();
    let local_path = directory.0.join("local");
    let peer_path = directory.0.join("peer");
    initialize_value(&local_path, b"local-history");
    initialize_value(&peer_path, b"conflicting-history");
    let local = Environment::open(&local_path).unwrap();
    let peer = Environment::open(&peer_path).unwrap();
    // A duplicated/forked origin can emit an identical later event. Comparing
    // just that event's digest does not establish its full preceding lineage.
    append(&local, 7, 2, b"common-last-event");
    append(&peer, 7, 2, b"common-last-event");
    let checkpoint = directory.0.join("forked-checkpoint");
    journal::export_snapshot(&peer, &peer.begin_read(), &checkpoint).unwrap();
    assert!(
        journal::rebase_snapshot(
            &checkpoint,
            &local,
            7,
            directory.0.join("candidate"),
            Limits::default(),
            project,
        )
        .is_err(),
        "a matching final event must not authorize a forked history"
    );
    assert_eq!(journal::cursor(&local.begin_read(), 7).unwrap().applied, 2);
    assert_eq!(
        journal::page(&local.begin_read(), 7, 0, Limits::default())
            .unwrap()
            .events[0]
            .payload,
        b"local-history"
    );
}

#[test]
fn recovery_uses_retention_floor_anchor_but_rejects_an_older_checkpoint() {
    let directory = Directory::new();
    std::fs::create_dir(&directory.0).unwrap();
    let local_path = directory.0.join("local");
    initialize(&local_path);
    let local = Environment::open(&local_path).unwrap();
    let baseline = directory.0.join("baseline");
    journal::export_snapshot(&local, &local.begin_read(), &baseline).unwrap();
    append(&local, 7, 2, b"retained-local-tail");
    let recovery = journal::export_snapshot(
        &local,
        &local.begin_read(),
        directory.0.join("current-recovery"),
    )
    .unwrap();
    let acknowledgements = [journal::PeerAck {
        peer: 8,
        origin: 7,
        sequence: 2,
    }];
    let mut write = local.begin_write().unwrap();
    let pruned = journal::prune(
        &mut write,
        &recovery,
        7,
        1,
        Limits::default(),
        &acknowledgements,
        1,
    )
    .unwrap();
    assert_eq!(pruned.floor, 1);
    write.commit().unwrap();

    // The checkpoint is exactly at the floor. Its cumulative lineage anchor
    // must survive removal of the corresponding event body.
    let rebased = journal::rebase_snapshot(
        &baseline,
        &local,
        7,
        directory.0.join("floor-candidate"),
        Limits::default(),
        project,
    )
    .unwrap();
    assert_eq!(
        journal::cursor(&rebased.begin_read(), 7).unwrap().applied,
        2
    );
    assert_eq!(
        rebased
            .begin_read()
            .get(APPLICATION, &7u64.to_be_bytes())
            .unwrap(),
        Some(b"retained-local-tail".as_slice())
    );

    let mut write = local.begin_write().unwrap();
    journal::prune(
        &mut write,
        &recovery,
        7,
        2,
        Limits::default(),
        &acknowledgements,
        1,
    )
    .unwrap();
    write.commit().unwrap();
    assert!(
        journal::rebase_snapshot(
            &baseline,
            &local,
            7,
            directory.0.join("too-old-candidate"),
            Limits::default(),
            project,
        )
        .is_err(),
        "an older checkpoint requires an event body already outside retention"
    );
    assert_eq!(journal::cursor(&local.begin_read(), 7).unwrap().floor, 2);
}
