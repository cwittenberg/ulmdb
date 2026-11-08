//! Business-CoW churn through the same optional-maintenance transfer lifecycle
//! used by short-lived C writers. No inventory prune/rebuild/compaction calls.
use super::*;
use crate::{MaintenanceTransfer, PublicationMode, ReadTxn, backup};
use std::{collections::BTreeSet, fs, path::PathBuf};

const ROWS: usize = 100_000;
const CHANGES: usize = 1_000;
const COMMIT_LIMIT: usize = 64;
const BYTES: u64 = 16 << 20;

struct Directory(PathBuf);
impl Directory {
    fn new() -> Self {
        Self(std::env::temp_dir().join(format!(
            "ulmdb-rewrite-churn-{}",
            ulmdb_storage::new_identity().unwrap()
        )))
    }
}
impl Drop for Directory {
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.0);
    }
}

fn key(n: usize) -> [u8; 16] {
    let mut key = [0; 16];
    key[..8].copy_from_slice(&(n as u64).to_be_bytes());
    key[8..].copy_from_slice(&(n as u64 ^ 0x9e37_79b9_7f4a_7c15).to_be_bytes());
    key
}
fn value(n: usize, version: u64) -> [u8; 64] {
    let mut value = [7; 64];
    value[..8].copy_from_slice(&(n as u64).to_be_bytes());
    value[56..].copy_from_slice(&version.to_be_bytes());
    value
}
fn policy() -> MaintenancePolicy {
    let MaintenancePolicy::CrossFormatResumable {
        rewrite_budget_bytes,
        reclaim,
        history_budget_bytes,
        ..
    } = MaintenancePolicy::AUTOMATIC
    else {
        panic!("automatic policy changed")
    };
    assert_eq!(MAX_VISITS, 1024);
    assert_eq!(MAX_COMPLETED, 1_048_576);
    assert_eq!(rewrite_budget_bytes, BYTES);
    MaintenancePolicy::CrossFormatResumable {
        segment_threshold: 2,
        rewrite_budget_bytes,
        reclaim,
        history_budget_bytes,
    }
}
fn seed(directory: &Directory) -> Environment {
    let env = Environment::create_with_publication(&directory.0, PublicationMode::Legacy).unwrap();
    env.set_maintenance_policy(MaintenancePolicy::Disabled)
        .unwrap();
    let mut write = env.begin_write().unwrap();
    write.create_database(b"rows", DatabaseKind::Kv).unwrap();
    write.create_database(b"cold", DatabaseKind::Kv).unwrap();
    assert_eq!(
        write
            .load_sorted_kv(b"rows", (0..ROWS).map(|n| Ok((key(n), value(n, 0)))))
            .unwrap(),
        ROWS as u64
    );
    write.put(b"cold", b"payload", &[0x5a; 8192]).unwrap();
    assert_eq!(write.commit().unwrap(), 1);
    let read = env.begin_read();
    let database = read.snapshot.catalog.get(b"rows".as_slice()).unwrap();
    let audit = ulmdb_kv::audit_tree(&read.snapshot.segments, database.root, 100_000).unwrap();
    assert!(
        audit.pages > MAX_VISITS as u64,
        "fixture must exceed one actual step: {audit:?}"
    );
    assert_eq!(audit.leaf_records, ROWS as u64);
    drop(read);
    env.set_maintenance_policy_with_fallback(policy()).unwrap();
    env
}
fn resume(directory: &Directory, transfer: MaintenanceTransfer) -> Environment {
    let env = Environment::open(&directory.0).unwrap();
    env.set_maintenance_policy(MaintenancePolicy::Disabled)
        .unwrap();
    env.restore_maintenance(transfer).unwrap();
    assert_eq!(env.maintenance_policy().unwrap(), policy());
    assert!(env.maintenance.lock().unwrap().fallback);
    env
}
fn next(random: &mut u64) -> u64 {
    *random ^= *random << 13;
    *random ^= *random >> 7;
    *random ^= *random << 17;
    *random
}
fn batch(random: &mut u64, version: u64) -> Vec<(usize, [u8; 64])> {
    let mut seen = BTreeSet::new();
    let mut rows = Vec::with_capacity(CHANGES);
    while rows.len() != CHANGES {
        let id = next(random) as usize % ROWS;
        if seen.insert(id) {
            rows.push((id, value(id, version)));
        }
    }
    rows
}
#[derive(Clone, Debug, PartialEq, Eq)]
struct ProofState {
    cutoff: u64,
    after: Option<Vec<u8>>,
    catalog: bool,
    complete: usize,
    recent: usize,
    mode: ProofMode,
    kept: usize,
}
fn proof(env: &Environment) -> Option<ProofState> {
    env.maintenance
        .lock()
        .unwrap()
        .rewrite
        .as_ref()
        .map(|p| ProofState {
            cutoff: p.cutoff,
            after: p.after.clone(),
            catalog: p.catalog,
            complete: p.complete.len(),
            recent: p.recent.len(),
            mode: p.proof_mode,
            kept: p.kept_segments.len(),
        })
}
fn prepared_identity(prepared: &crate::PreparedWriteTxn<'_>) -> (u64, u128, u128, u128) {
    let crate::prepared::PreparedState::Publish { snapshot, .. } = &prepared.state else {
        panic!("random row changes must publish")
    };
    let read = snapshot.read();
    let m = read.file_manifest().unwrap();
    (m.sequence, m.commit, m.parent, m.previous)
}
fn prepare<'a>(env: &'a Environment, rows: &[(usize, [u8; 64])]) -> crate::PreparedWriteTxn<'a> {
    let mut write = env.begin_write().unwrap();
    for (id, value) in rows {
        write.put(b"rows", &key(*id), value).unwrap();
    }
    write.prepare().unwrap()
}
fn assert_work_budget(before: &MaintenanceStatus, after: &MaintenanceStatus) {
    assert!(
        after.incremental_rewrite_objects - before.incremental_rewrite_objects <= MAX_VISITS as u64
    );
    // Every copied byte has paid the existing read+copy charge.
    assert!(after.incremental_rewrite_bytes - before.incremental_rewrite_bytes <= BYTES / 2);
    assert_eq!(after.last_fallback_rewrite_error, None, "{after:?}");
}
fn check_rows(read: &ReadTxn, versions: &[u64]) {
    let mut count = 0;
    for row in read
        .range(b"rows", Bound::Unbounded, Bound::Unbounded, false)
        .unwrap()
    {
        let (actual_key, actual_value) = row.unwrap();
        assert_eq!(actual_key, key(count));
        assert_eq!(actual_value, value(count, versions[count]));
        count += 1;
    }
    assert_eq!(count, ROWS);
    assert_eq!(
        read.get(b"cold", b"payload").unwrap().unwrap(),
        &[0x5a; 8192]
    );
}

#[test]
fn random_business_churn_finishes_evacuating_with_fixed_budgets_and_transferred_owners() {
    let directory = Directory::new();
    let mut env = seed(&directory);
    let held = env.begin_read();
    let payload = held.get(b"cold", b"payload").unwrap().unwrap();
    let old_segments: BTreeSet<_> = held
        .file_manifest()
        .unwrap()
        .segments
        .iter()
        .map(|e| e.id)
        .collect();
    let mut versions = vec![0; ROWS];
    let mut random = 0xd4e7_190a_f2c6_835bu64;

    // The first attempt creates real proofs and candidate files, then aborts.
    // No selected proof exists yet, so installing any of them is observable.
    let aborted = batch(&mut random, 1);
    let selected = env.file_checkpoint().unwrap();
    let before = env.maintenance_status().unwrap();
    let prepared = prepare(&env, &aborted);
    let staged = prepared
        .maintenance_rewrite
        .as_ref()
        .expect("large graph requires a continuation");
    assert!(staged.complete.len() > 0);
    assert!(
        proof(&env).is_none(),
        "preparation installed private proofs"
    );
    prepared.abort();
    assert_eq!(env.file_checkpoint().unwrap(), selected);
    assert!(
        proof(&env).is_none(),
        "aborted candidate installed its proofs"
    );
    assert_work_budget(&before, &env.maintenance_status().unwrap());
    check_rows(&env.begin_read(), &versions);

    let mut peak_complete = 0;
    let mut peak_recent = 0;
    let mut peak_combined = 0;
    let mut peak_kept = 0;
    let mut peak_inventory = 0;
    let mut finished = None;
    for turn in 0..COMMIT_LIMIT {
        let transfer = env.take_maintenance().unwrap();
        drop(env);
        env = resume(&directory, transfer);
        let previous = env.begin_read();
        let m = previous.file_manifest().unwrap();
        let previous_identity = (m.sequence, m.commit, m.generation);
        let rows = batch(&mut random, turn as u64 + 2);
        let status_before = env.maintenance_status().unwrap();
        let prepared = prepare(&env, &rows);
        let identity = prepared_identity(&prepared);
        assert_eq!(identity.0, previous_identity.0 + 1);
        assert_eq!(
            (identity.2, identity.3),
            (previous_identity.1, previous_identity.2)
        );
        assert_eq!(prepared.commit().unwrap(), identity.0);
        for (id, _) in &rows {
            versions[*id] = turn as u64 + 2;
        }
        let status = env.maintenance_status().unwrap();
        assert_work_budget(&status_before, &status);
        assert!(status.fallback_rewrite_attempts > 0);
        let current = env.begin_read();
        let m = current.file_manifest().unwrap();
        assert_eq!((m.sequence, m.commit, m.parent, m.previous), identity);
        let actual_previous = ReadTxn::open_previous_coordinated_snapshot(
            &directory.0,
            crate::ResidencyPolicy::Demand,
        )
        .unwrap();
        assert_eq!(
            (
                actual_previous.sequence(),
                actual_previous.commit_identity(),
                actual_previous.generation()
            ),
            previous_identity
        );
        assert_eq!(
            actual_previous.get(b"rows", &key(rows[0].0)).unwrap(),
            previous.get(b"rows", &key(rows[0].0)).unwrap()
        );
        assert_eq!(payload, &[0x5a; 8192]);
        assert_eq!(
            held.get(b"rows", &key(rows[0].0)).unwrap().unwrap(),
            value(rows[0].0, 0)
        );
        peak_inventory = peak_inventory.max(m.segments.len());
        if let Some(state) = proof(&env) {
            peak_complete = peak_complete.max(state.complete);
            peak_recent = peak_recent.max(state.recent);
            peak_combined = peak_combined.max(state.complete + state.recent);
            peak_kept = peak_kept.max(state.kept);
            assert!(state.complete + state.recent <= MAX_COMPLETED && state.recent <= MAX_RECENT && state.kept <= MAX_COMPLETED);
            assert!(
                state.complete + state.recent <= (turn + 1) * MAX_VISITS,
                "uncharged proof growth: {state:?}"
            );
        }
        let removed = m
            .segments
            .iter()
            .all(|entry| !old_segments.contains(&entry.id));
        if removed {
            assert!(
                proof(&env).is_none(),
                "completed cycle retained its frontier"
            );
            assert!(status.rebuilt_candidates > 0);
            check_rows(&current, &versions);
            finished = Some(turn + 1);
            break;
        }
    }
    eprintln!(
        "REWRITE_CHURN commits={finished:?} peak_complete={peak_complete} peak_recent={peak_recent} peak_combined={peak_combined} peak_kept={peak_kept} peak_inventory={peak_inventory} visits_per_step={MAX_VISITS} bytes_per_step={BYTES}"
    );
    assert!(
        finished.is_some(),
        "continuous random CoW updates starved evacuation within {COMMIT_LIMIT} commits: {:?}",
        env.maintenance_status().unwrap()
    );
    assert!(peak_combined >= MAX_VISITS / 2 && peak_combined < MAX_COMPLETED);
    assert_eq!(peak_kept, 0);
    assert_eq!(payload, &[0x5a; 8192]);
    check_rows(&held, &vec![0; ROWS]);
    let transfer = env.take_maintenance().unwrap();
    drop(env);
    let reopened = resume(&directory, transfer);
    check_rows(&reopened.begin_read(), &versions);
}

#[test]
fn prepared_abort_preserves_existing_committed_proofs_across_transfer() {
    let directory = Directory::new();
    let mut env = seed(&directory);
    let mut random = 0x7350_18b4_c9e6_a2df;
    let first = batch(&mut random, 1);
    prepare(&env, &first).commit().unwrap();
    let installed = proof(&env).expect("large graph needs committed continuation");
    assert!(installed.complete + installed.recent > 0);
    let transfer = env.take_maintenance().unwrap();
    drop(env);
    env = resume(&directory, transfer);
    assert_eq!(proof(&env), Some(installed.clone()));
    let selected = env.file_checkpoint().unwrap();
    let before = env.begin_read();
    let rows = batch(&mut random, 2);
    let prepared = prepare(&env, &rows);
    if let Some(next) = &prepared.maintenance_rewrite {
        assert!(next.complete.len() + next.recent.len() > installed.complete + installed.recent);
    }
    assert_eq!(
        proof(&env),
        Some(installed.clone()),
        "preparation replaced selected proofs"
    );
    prepared.abort();
    assert_eq!(
        proof(&env),
        Some(installed),
        "abort installed private proofs or cleared committed ones"
    );
    assert_eq!(env.file_checkpoint().unwrap(), selected);
    for (id, _) in &rows {
        assert_eq!(
            env.begin_read().get(b"rows", &key(*id)).unwrap(),
            before.get(b"rows", &key(*id)).unwrap()
        );
    }
}

#[test]
fn imported_and_nonforward_roots_reset_completed_rewrite_coverage() {
    let directory = Directory::new();
    let branch = Directory::new();
    let env = seed(&directory);
    let mut random = 0xa91c_d507_2e84_63bf;
    prepare(&env, &batch(&mut random, 1)).commit().unwrap();
    assert!(proof(&env).is_some());
    let source = env.begin_read();
    backup::export(&env, &branch.0).unwrap();
    let other = Environment::open(&branch.0).unwrap();
    other
        .set_maintenance_policy(MaintenancePolicy::Disabled)
        .unwrap();
    let rows = batch(&mut random, 2);
    let mut write = other.begin_write().unwrap();
    for (id, value) in &rows {
        write.put(b"rows", &key(*id), value).unwrap();
    }
    let target_sequence = write.commit().unwrap();
    let mut delta = Vec::new();
    other
        .dump_incremental(source.sequence(), &mut delta)
        .unwrap();
    env.load_incremental(&mut delta.as_slice()).unwrap();
    assert!(
        proof(&env).is_none(),
        "import reused business-forward proofs"
    );
    assert_eq!(env.begin_read().sequence(), target_sequence);
    assert_eq!(
        env.begin_read().commit_identity(),
        other.begin_read().commit_identity()
    );
    for (id, value) in &rows {
        assert_eq!(
            env.begin_read().get(b"rows", &key(*id)).unwrap().unwrap(),
            value
        );
    }
    let rows = batch(&mut random, 3);
    let sequence = prepare(&env, &rows).commit().unwrap();
    assert!(proof(&env).is_some());
    env.rollback_last_commit(sequence).unwrap();
    assert!(
        proof(&env).is_none(),
        "rollback retained forward completed proofs"
    );
    assert_eq!(env.begin_read().sequence(), target_sequence);
    assert_eq!(
        env.begin_read().commit_identity(),
        other.begin_read().commit_identity()
    );
}

#[test]
fn transfer_discards_changed_checkpoint_proofs_and_rejects_foreign_realm() {
    let directory = Directory::new();
    let env = seed(&directory);
    let mut random = 0x138d_26a4_f7b0_c95e;
    prepare(&env, &batch(&mut random, 1)).commit().unwrap();
    assert!(proof(&env).is_some());
    let transfer = env.take_maintenance().unwrap();
    drop(env);
    let env = Environment::open(&directory.0).unwrap();
    env.set_maintenance_policy(MaintenancePolicy::Disabled)
        .unwrap();
    let rows = batch(&mut random, 2);
    let sequence = prepare(&env, &rows).commit().unwrap();
    env.restore_maintenance(transfer).unwrap();
    assert!(
        proof(&env).is_none(),
        "unobserved checkpoint retained proofs"
    );
    assert_eq!(env.begin_read().sequence(), sequence);
    for (id, value) in &rows {
        assert_eq!(
            env.begin_read().get(b"rows", &key(*id)).unwrap().unwrap(),
            value
        );
    }

    prepare(&env, &batch(&mut random, 3)).commit().unwrap();
    assert!(proof(&env).is_some());
    let transfer = env.take_maintenance().unwrap();
    drop(env);
    let foreign = Directory::new();
    let other = Environment::create(&foreign.0).unwrap();
    other
        .set_maintenance_policy(MaintenancePolicy::Disabled)
        .unwrap();
    let checkpoint = other.file_checkpoint().unwrap();
    assert!(matches!(
        other.restore_maintenance(transfer),
        Err(Error::InvalidInput("foreign maintenance transfer"))
    ));
    assert!(proof(&other).is_none());
    assert_eq!(other.file_checkpoint().unwrap(), checkpoint);
}


