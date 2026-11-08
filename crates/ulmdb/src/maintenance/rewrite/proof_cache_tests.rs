//! Proposed native unit module. SOURCE ONLY until root applies and schedules it.
//! Controls are exact historical walkers, not a mirrored algorithm/model.
use super::*;
use crate::{PublicationMode, ReadTxn};
use std::{collections::BTreeSet, path::PathBuf};

#[path = "proof_cache_tests/controls/old_minimal.rs"]
mod old_minimal;
#[path = "proof_cache_tests/controls/v7_retained.rs"]
mod v7_retained;

const SIZE: usize = 256;
const VISITS: usize = 6;
const CALLS: usize = 64;
struct Directory(PathBuf);
impl Directory {
    fn new() -> Self {
        let path = std::env::temp_dir().join(format!(
            "ulmdb-proof-cap-{}", ulmdb_storage::new_identity().unwrap()
        ));
        ulmdb_storage::create_layout(&path).unwrap();
        Self(path)
    }
}
impl Drop for Directory {
    fn drop(&mut self) { let _ = std::fs::remove_dir_all(&self.0); }
}
fn key(id: usize) -> [u8; 8] { (id as u64).to_be_bytes() }
fn value(id: usize, version: u64) -> [u8; 32] {
    let mut value = [0x5a; 32];
    value[..8].copy_from_slice(&(id as u64).to_be_bytes());
    value[8..16].copy_from_slice(&version.to_be_bytes());
    value
}

// Independent small V3 encoder; the production decoder/auditor must accept it.
// This chooses exact fanout rather than depending on the ordinary split policy.
fn page(arena: &mut Arena, level: u16, first: Reference, cells: &[Vec<u8>]) -> Reference {
    let mut bytes = [0; SIZE];
    bytes[..2].copy_from_slice(&(if level == 0 { 2u16 } else { 3 }).to_le_bytes());
    bytes[2..4].copy_from_slice(&3u16.to_le_bytes());
    bytes[4..6].copy_from_slice(&level.to_le_bytes());
    bytes[6..8].copy_from_slice(&(cells.len() as u16).to_le_bytes());
    bytes[8..10].copy_from_slice(&((32 + 2 * cells.len()) as u16).to_le_bytes());
    bytes[12..16].copy_from_slice(&(SIZE as u32).to_le_bytes());
    bytes[16..32].copy_from_slice(&first.encode());
    let mut upper = SIZE;
    for (index, cell) in cells.iter().enumerate() {
        assert_eq!(cell.len() % 2, 0);
        upper -= cell.len();
        assert!(upper >= 32 + 2 * cells.len());
        bytes[upper..upper + cell.len()].copy_from_slice(cell);
        bytes[32 + 2 * index..34 + 2 * index].copy_from_slice(&(upper as u16).to_le_bytes());
    }
    bytes[10..12].copy_from_slice(&(upper as u16).to_le_bytes());
    arena.allocate(&bytes, SIZE).unwrap()
}
fn leaf(arena: &mut Arena, id: Option<usize>) -> Reference {
    let cells = id.map(|id| {
        let mut cell = vec![0; 48];
        cell[..2].copy_from_slice(&8u16.to_le_bytes());
        cell[4..8].copy_from_slice(&32u32.to_le_bytes());
        cell[8..16].copy_from_slice(&key(id));
        cell[16..].copy_from_slice(&value(id, 0));
        vec![cell]
    }).unwrap_or_default();
    page(arena, 0, Reference::NULL, &cells)
}
fn branch(arena: &mut Arena, level: u16, first: Reference, rest: &[(usize, Reference)]) -> Reference {
    let cells: Vec<_> = rest.iter().map(|(id, child)| {
        let mut cell = vec![0; 28];
        cell[..2].copy_from_slice(&8u16.to_le_bytes());
        cell[4..20].copy_from_slice(&child.encode());
        cell[20..].copy_from_slice(&key(*id));
        cell
    }).collect();
    page(arena, level, first, &cells)
}
struct Fixture {
    directory: Directory,
    base: ulmdb_storage::SegmentTable,
    root: Reference,
    all: BTreeSet<Reference>,
    extra: Vec<Reference>,
    payload: Reference,
    versions: Vec<Option<u64>>,
}
impl Fixture {
    fn new(alias: bool) -> Self {
        let directory = Directory::new();
        let base = ulmdb_storage::SegmentTable::default()
            .with_page_size(ulmdb_core::PageSize::new(SIZE).unwrap()).unwrap();
        let mut arena = Arena::new(base, 1).unwrap();
        let mut all = BTreeSet::new();
        let leaves: Vec<_> = (0..16).map(|id| {
            let at = leaf(&mut arena, Some(id)); all.insert(at); at
        }).collect();
        let mut branches: Vec<_> = (0..4).map(|group| {
            let start = 4 * group;
            let rest: Vec<_> = (1..4).map(|j| (start + j, leaves[start + j])).collect();
            let at = branch(&mut arena, 1, leaves[start], &rest); all.insert(at); at
        }).collect();
        let mut versions = vec![Some(0); 16];
        if alias {
            // A shared empty unary subtree is role- and separator-valid at
            // multiple bounds; aliasing nonempty rows across ranges is not.
            let empty = leaf(&mut arena, None); all.insert(empty);
            let shared = branch(&mut arena, 1, empty, &[]); all.insert(shared);
            branches[1] = shared; branches[2] = shared;
            versions[4..12].fill(None);
        }
        let root = branch(&mut arena, 2, branches[0], &[(4, branches[1]), (8, branches[2]), (12, branches[3])]);
        all.insert(root);
        let audit = ulmdb_kv::audit_tree(&arena, root, 100).unwrap();
        assert_eq!(audit.height, 3);
        assert!(audit.pages > 8);
        if !alias { assert_eq!(audit.pages, 21); }
        let extra: Vec<_> = (100..108).map(|id| {
            let at = leaf(&mut arena, Some(id)); all.insert(at); at
        }).collect();
        let payload = arena.put_payload(&[0x6d; 12_288]).unwrap();
        let (base, _) = arena.seal(&directory.0).unwrap();
        Self { directory, base, root, all, extra, payload, versions }
    }
    fn collect_new(&mut self, arena: &Arena, segment: u64) {
        // Walk/change allocate only fixed-size KV pages in these cases.
        for offset in (0..arena.dirty_bytes()).step_by(SIZE) {
            self.all.insert(Reference { segment, offset: offset as u64 });
        }
    }
}
fn check<S: ReadStore>(store: &S, root: Reference, versions: &[Option<u64>]) {
    let audit = ulmdb_kv::audit_tree(store, root, 1024).unwrap();
    assert_eq!(audit.leaf_records, versions.iter().filter(|v| v.is_some()).count() as u64);
    for reverse in [false, true] {
        let got: Vec<_> = ulmdb_kv::Cursor::new(store, root, Bound::Unbounded, Bound::Unbounded, reverse)
            .unwrap().map(|row| { let (k,v)=row.unwrap(); (k.to_vec(),v.to_vec()) }).collect();
        let mut want: Vec<_> = versions.iter().enumerate().filter_map(|(id, version)|
            version.map(|version| (key(id).to_vec(), value(id, version).to_vec()))).collect();
        if reverse { want.reverse(); }
        assert_eq!(got, want);
    }
}
#[derive(Clone)]
enum Driver { Actual(Progress), Minimal(old_minimal::Driver), Retained(v7_retained::Driver) }
impl Driver {
    fn actual(total: usize, recent: usize, cutoff: u64) -> Self {
        Self::Actual(Progress { cutoff, test_limits: Some((total, recent)), ..Default::default() })
    }
    fn counts(&self) -> (usize, usize) {
        match self {
            Self::Actual(p) => (p.complete.len(), p.recent.len()),
            Self::Minimal(p) => (p.count(), 0), Self::Retained(p) => (p.count(), 0),
        }
    }
    fn step(&mut self, arena: &mut Arena, root: Reference, visits: usize) -> (Reference, bool, usize, usize) {
        match self {
            Self::Actual(p) => {
                let mut budget = Budget { allowance:16<<20, bytes:16<<20, visits, copied:0, inspected:0 };
                let count = p.complete.len() + p.recent.len();
                if !p.begin_step(&mut budget) {
                    let remaining = p.complete.len() + p.recent.len();
                    assert!(count - remaining <= visits);
                    assert_eq!(budget.inspected, 0); assert_eq!(budget.copied, 0);
                    assert_eq!(arena.dirty_bytes(), 0, "drain touched graph storage");
                    return (root, false, 0, count - remaining);
                }
                let (next, done) = p.walk(arena, root, Kind::Kv, &mut budget, 0, 0).unwrap();
                assert!(budget.inspected <= visits); assert!(budget.copied <= 8<<20);
                (next, p.finish_step(done), budget.inspected, 0)
            }
            Self::Minimal(p) => { let (r,d,n)=p.step(arena,root,visits).unwrap(); (r,d,n,0) }
            Self::Retained(p) => { let (r,d,n)=p.step(arena,root,visits).unwrap(); (r,d,n,0) }
        }
    }
}
fn finish(mut driver: Driver, mut fixture: Fixture, total: usize, recent: usize) -> (bool, usize) {
    let held = fixture.base.clone(); let held_root = fixture.root;
    let held_versions = fixture.versions.clone();
    let first_segment=fixture.base.entries().iter().map(|e|e.id).max().unwrap_or(0)+1;
    for call in 0..CALLS {
        let segment=call as u64+first_segment;
        let mut arena=Arena::new(fixture.base.clone(),segment).unwrap();
        let (root,done,_,_) = driver.step(&mut arena,fixture.root,VISITS);
        let (primary,cached)=driver.counts();
        assert!(primary+cached<=total && cached<=recent);
        check(&arena,root,&fixture.versions); check(&held,held_root,&held_versions);
        assert_eq!(held.payload(fixture.payload).unwrap(), &[0x6d;12_288]);
        fixture.collect_new(&arena,segment);
        let (base,_) = arena.seal(&fixture.directory.0).unwrap();
        fixture.base=base;fixture.root=root;
        if done { return (true,call+1); }
    }
    (false,CALLS)
}

#[test]
fn static_graph_above_cap_finishes_original_and_proposal_but_saturates_v7() {
    assert!(finish(Driver::Minimal(old_minimal::Driver::new(2)),Fixture::new(false),8,0).0);
    assert!(!finish(Driver::Retained(v7_retained::Driver::new(2)),Fixture::new(false),8,0).0);
    assert!(finish(Driver::actual(8,2,2),Fixture::new(false),8,2).0);
    let mut minimal=Driver::actual(8,0,2);
    if let Driver::Actual(p)=&mut minimal {p.proof_mode=ProofMode::Minimal;}
    assert!(finish(minimal,Fixture::new(false),8,0).0);
}

#[test]
fn role_valid_shared_empty_subtree_preserves_aliases_and_finishes() {
    // cutoff1 leaves the shared objects in place, exercising actual cache hits
    // on one composite reference reached through two distinct parent slots.
    assert!(finish(Driver::Minimal(old_minimal::Driver::new(1)),Fixture::new(true),8,0).0);
    assert!(finish(Driver::actual(8,2,1),Fixture::new(true),8,2).0);
}

#[test]
fn completed_children_still_avoid_full_subtree_reads_when_recent_cache_fits() {
    let fixture=Fixture::new(false);
    let mut actual=Progress {cutoff:2,test_limits:Some((32,24)),..Default::default()};
    let mut warm=Arena::new(fixture.base.clone(),2).unwrap();
    let mut budget=Budget {allowance:16<<20,bytes:16<<20,visits:64,copied:0,inspected:0};
    let (root,done)=actual.walk(&mut warm,fixture.root,Kind::Kv,&mut budget,0,0).unwrap();
    assert!(done);assert!(actual.recent.len()>0);
    let (base,_)=warm.seal(&fixture.directory.0).unwrap();
    let mut business=Arena::new(base.clone(),3).unwrap();
    let changed=ulmdb_kv::put(&mut business,root,&key(15),&value(15,1)).unwrap();
    let mut budget=Budget {allowance:16<<20,bytes:16<<20,visits:VISITS,copied:0,inspected:0};
    let (next,done)=actual.walk(&mut business,changed,Kind::Kv,&mut budget,0,0).unwrap();
    assert!(done && budget.inspected<=4);assert_eq!(budget.copied,0);assert_eq!(next,changed);
    let mut versions=fixture.versions.clone();versions[15]=Some(1);
    check(&business,next,&versions);check(&base,root,&fixture.versions);
}

#[test]
fn finite_stale_cow_edits_remain_bounded_then_quiescent_graph_finishes() {
    let mut fixture=Fixture::new(false);
    let held=fixture.base.clone();let original=fixture.root;let old=fixture.versions.clone();
    let mut driver=Driver::actual(64,24,2);
    for turn in 0..16 {
        let segment=2+turn*2;
        let mut arena=Arena::new(fixture.base.clone(),segment).unwrap();
        let (root,_,_,_)=driver.step(&mut arena,fixture.root,VISITS);
        let (base,_)=arena.seal(&fixture.directory.0).unwrap();
        let mut business=Arena::new(base,segment+1).unwrap();
        let id=(turn as usize*5)%16;
        let root=ulmdb_kv::put(&mut business,root,&key(id),&value(id,turn+1)).unwrap();
        fixture.versions[id]=Some(turn+1);
        check(&business,root,&fixture.versions);check(&held,original,&old);
        let (primary,recent)=driver.counts();assert!(primary+recent<=64 && recent<=24);
        let (base,_)=business.seal(&fixture.directory.0).unwrap();
        fixture.base=base;fixture.root=root;
    }
    // Avoid reusing a segment ID already allocated by the stale-write trace.
    // The finishing helper normally starts at2, so drive this continuation here.
    let mut done=false;
    for step in 0..CALLS {
        let mut arena=Arena::new(fixture.base.clone(),100+step as u64).unwrap();
        let (root,finished,_,_)=driver.step(&mut arena,fixture.root,VISITS);
        check(&arena,root,&fixture.versions);check(&held,original,&old);
        let (primary,recent)=driver.counts();assert!(primary+recent<=64 && recent<=24);
        let (base,_)=arena.seal(&fixture.directory.0).unwrap();fixture.base=base;fixture.root=root;
        if finished {done=true;break;}
    }
    assert!(done,"finite churn left a static graph unable to finish");
}

fn fingerprint(p:&Progress,all:&BTreeSet<Reference>) -> Vec<(bool,bool)> {
    all.iter().map(|at|(p.complete.contains(&(*at,Kind::Kv)),p.recent.contains(&(*at,Kind::Kv)))).collect()
}

#[test]
fn forced_saturation_and_drain_are_clone_local_and_keep_cutoff_and_preserved_segments() {
    let mut fixture=Fixture::new(true);
    let mut p=Progress {cutoff:2,test_limits:Some((8,2)),..Default::default()};
    let mut arena=Arena::new(fixture.base.clone(),2).unwrap();
    // Establish an actual conservative old-payload preservation proof first.
    let mut small=Budget {allowance:512,bytes:512,visits:VISITS,copied:0,inspected:0};
    assert!(p.walk(&mut arena,fixture.payload,Kind::Payload,&mut small,0,0).unwrap().1);
    let kept=(Reference {segment:fixture.payload.segment,offset:0},Kind::Payload);
    assert!(p.kept_segments.contains(&kept));
    // Real typed proofs for valid immutable but currently unreachable objects,
    // as can remain after earlier business CoW versions. No fake proof keys.
    for at in fixture.extra.iter().take(7) {
        let mut b=Budget {allowance:16<<20,bytes:16<<20,visits:VISITS,copied:0,inspected:0};
        assert!(p.walk(&mut arena,*at,Kind::Kv,&mut b,0,0).unwrap().1);
    }
    assert_eq!(p.complete.len(),8);assert_eq!(p.recent.len(),0);
    fixture.collect_new(&arena,2);
    let (base,_)=arena.seal(&fixture.directory.0).unwrap();fixture.base=base;
    let mut arena=Arena::new(fixture.base.clone(),3).unwrap();
    p.after=Some(b"preserve-until-selection".to_vec());p.catalog=true;
    let before=fingerprint(&p,&fixture.all);
    let mut candidate=p.clone();
    let checkpoint=arena.checkpoint().unwrap();
    let mut b=Budget {allowance:16<<20,bytes:16<<20,visits:VISITS,copied:0,inspected:0};
    let (root,done)=candidate.walk(&mut arena,fixture.root,Kind::Kv,&mut b,0,0).unwrap();
    assert!(candidate.proof_mode==ProofMode::RestartRequested);
    assert!(!candidate.finish_step(done),"restart authorized inventory projection");
    assert!(candidate.proof_mode==ProofMode::Drain);
    assert!(candidate.after.is_none() && !candidate.catalog);
    check(&arena,root,&fixture.versions);
    // Abandoning the prepared clone cannot install mode, tree or traversal changes.
    drop(candidate);
    arena.rollback_checkpoint(&checkpoint).unwrap();
    assert!(p.proof_mode==ProofMode::Reuse);assert_eq!(fingerprint(&p,&fixture.all),before);
    assert!(p.complete.contains(&(fixture.payload,Kind::Payload)));
    assert!(p.catalog && p.after.as_deref()==Some(b"preserve-until-selection".as_slice()));
    let mut candidate=p.clone();
    let mut b=Budget {allowance:16<<20,bytes:16<<20,visits:VISITS,copied:0,inspected:0};
    let (root,done)=candidate.walk(&mut arena,fixture.root,Kind::Kv,&mut b,0,0).unwrap();
    assert!(!candidate.finish_step(done));p=candidate; // selected clone only
    fixture.collect_new(&arena,3);
    let (base,_)=arena.seal(&fixture.directory.0).unwrap();fixture.base=base;fixture.root=root;
    let cutoff=p.cutoff;
    let count=p.complete.len()+p.recent.len();let mut calls=0;
    while p.proof_mode==ProofMode::Drain {
        let before=p.complete.len()+p.recent.len();
        let mut candidate=p.clone();
        let mut b=Budget {allowance:16<<20,bytes:16<<20,visits:VISITS,copied:0,inspected:0};
        assert!(!candidate.begin_step(&mut b));
        assert!(before-candidate.complete.len()-candidate.recent.len()<=VISITS);
        assert_eq!((b.inspected,b.copied),(0,0));
        assert_eq!(p.complete.len()+p.recent.len(),before,"private drain mutated committed version");
        p=candidate;calls+=1;
    }
    assert!(calls<=count.div_ceil(VISITS));assert_eq!(p.cutoff,cutoff);
    assert!(p.proof_mode==ProofMode::Minimal && p.recent.len()==0);
    assert!(p.kept_segments.contains(&kept));
    assert!(finish(Driver::Actual(p),fixture,8,0).0);
}

#[test]
fn actual_prepared_restart_and_drain_install_only_after_selection_without_early_projection() {
    let path=std::env::temp_dir().join(format!("ulmdb-proof-selection-{}",ulmdb_storage::new_identity().unwrap()));
    struct Remove(PathBuf);impl Drop for Remove {fn drop(&mut self){let _=std::fs::remove_dir_all(&self.0);}}
    let _remove=Remove(path.clone());
    let env=Environment::create_with_publication(&path,PublicationMode::Legacy).unwrap();
    env.set_maintenance_policy(MaintenancePolicy::Disabled).unwrap();
    let mut w=env.begin_write().unwrap();w.create_database(b"rows",DatabaseKind::Kv).unwrap();
    for id in 0..16 {w.put(b"rows",&key(id),&value(id,0)).unwrap();}w.commit().unwrap();
    let held=env.begin_read();let payload=held.get(b"rows",&key(7)).unwrap().unwrap();
    let original: BTreeSet<_>=held.file_manifest().unwrap().segments.iter().map(|e|e.id).collect();
    let cutoff=held.file_manifest().unwrap().next_segment;
    let MaintenancePolicy::CrossFormatResumable {rewrite_budget_bytes,reclaim,history_budget_bytes,..}=MaintenancePolicy::AUTOMATIC else {panic!()};
    env.set_maintenance_policy_with_fallback(MaintenancePolicy::CrossFormatResumable {
        segment_threshold:2,rewrite_budget_bytes,reclaim,history_budget_bytes}).unwrap();
    env.maintenance.lock().unwrap().rewrite=Some(Progress {cutoff,test_limits:Some((1,0)),..Default::default()});
    let prepare=|version| {let mut w=env.begin_write().unwrap();w.put(b"rows",&key(7),&value(7,version)).unwrap();w.prepare().unwrap()};
    let checkpoint=env.file_checkpoint().unwrap();
    let p=prepare(1);assert!(p.maintenance_rewrite.as_ref().unwrap().proof_mode==ProofMode::Drain);
    assert!(env.maintenance.lock().unwrap().rewrite.as_ref().unwrap().proof_mode==ProofMode::Reuse);
    p.abort();assert_eq!(env.file_checkpoint().unwrap(),checkpoint);
    assert!(env.maintenance.lock().unwrap().rewrite.as_ref().unwrap().proof_mode==ProofMode::Reuse);
    prepare(1).commit().unwrap();
    assert!(env.maintenance.lock().unwrap().rewrite.as_ref().unwrap().proof_mode==ProofMode::Drain);
    assert!(env.begin_read().file_manifest().unwrap().segments.iter().any(|e|original.contains(&e.id)),"restart projected inventory early");
    let before=env.maintenance_status().unwrap();let checkpoint=env.file_checkpoint().unwrap();
    let p=prepare(2);assert!(p.maintenance_rewrite.as_ref().unwrap().proof_mode==ProofMode::Minimal);
    p.abort();assert_eq!(env.file_checkpoint().unwrap(),checkpoint);
    assert!(env.maintenance.lock().unwrap().rewrite.as_ref().unwrap().proof_mode==ProofMode::Drain);
    prepare(2).commit().unwrap();let after=env.maintenance_status().unwrap();
    assert_eq!(after.incremental_rewrite_objects,before.incremental_rewrite_objects);
    assert_eq!(after.incremental_rewrite_bytes,before.incremental_rewrite_bytes);
    assert!(env.maintenance.lock().unwrap().rewrite.as_ref().unwrap().proof_mode==ProofMode::Minimal);
    assert!(env.begin_read().file_manifest().unwrap().segments.iter().any(|e|original.contains(&e.id)),"drain projected inventory");
    prepare(3).commit().unwrap();
    assert!(env.maintenance.lock().unwrap().rewrite.is_none());
    assert!(env.begin_read().file_manifest().unwrap().segments.iter().all(|e|!original.contains(&e.id)));
    assert_eq!(env.begin_read().get(b"rows",&key(7)).unwrap().unwrap(),value(7,3));
    assert_eq!(payload,value(7,0));
}
