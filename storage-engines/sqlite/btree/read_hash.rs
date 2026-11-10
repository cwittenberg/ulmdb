//! Optional exact lookup over slices owned by one actual native ReadTxn.
//! Index construction never changes the store, its snapshot, or its values.
use super::{native, code, lower, upper, Val, Key, Result, DATA, IO, MAX_ROW};
use std::{cell::RefCell, collections::hash_map::RandomState, hash::{BuildHasher, Hash, Hasher}, mem::size_of, ops::Bound};

const REQUESTS: u8 = 64;
const MAX_PROBES: usize = 64;
const DB_CHARGE: usize = 256;
#[derive(Clone, Copy)]
struct Limits { entries: usize, bytes: usize, values: usize, databases: usize }
const LIMITS: Limits = Limits { entries: 65_536, bytes: 8 << 20, values: 64 << 20, databases: 1024 };
type View<'a> = (&'a [u8], &'a [u8]);
struct Index<'a> { slots: Box<[Option<View<'a>>]>, hash: RandomState }
enum Database<'a> { Warming(u8), Ordered, Failed(i32), Indexed(Index<'a>) }
struct State<'a> {
    databases: Vec<(u32, Database<'a>)>, entries: usize, bytes: usize, values: usize,
    limits: Limits,
}
pub(super) struct Lookup<'a> { view: native::KvRead<'a>, state: RefCell<State<'a>>,
    #[cfg(feature = "btree-blocks")]
    read: &'a native::ReadTxn,
    #[cfg(feature = "btree-blocks")]
    stores: RefCell<Vec<(u32, super::blocks::SnapshotRows<'a>)>>,
    #[cfg(all(feature = "btree-blocks", test))]
    audits: std::cell::Cell<usize>,
}
enum Prepared<'a> { Unavailable, Ready { index: Index<'a>, entries: usize, bytes: usize, values: usize } }
enum Probe<'a> { Value(&'a [u8]), Missing, Ordered }
fn hash(state: &RandomState, key: &[u8]) -> usize {
    let mut hasher = state.build_hasher(); key.hash(&mut hasher); hasher.finish() as usize
}
impl<'a> Index<'a> {
    fn insert(&mut self, key: &'a [u8], value: &'a [u8]) -> Result<bool> {
        let mask = self.slots.len() - 1;
        let start = hash(&self.hash, key);
        for distance in 0..MAX_PROBES.min(self.slots.len()) {
            let slot = &mut self.slots[start.wrapping_add(distance) & mask];
            match slot {
                None => { *slot = Some((key, value)); return Ok(true); }
                Some((old, _)) if *old == key => return Err(IO),
                _ => {}
            }
        }
        Ok(false)
    }
    fn get(&self, key: &[u8]) -> Probe<'a> {
        let mask = self.slots.len() - 1;
        let start = hash(&self.hash, key);
        for distance in 0..MAX_PROBES.min(self.slots.len()) {
            match self.slots[start.wrapping_add(distance) & mask] {
                None => return Probe::Missing,
                Some((stored, value)) if stored == key => return Probe::Value(value),
                _ => {}
            }
        }
        Probe::Ordered
    }
}
fn validate(dbi: u32, key: &[u8], value: &[u8]) -> Result<()> {
    if key.len() != 16 || key[..8] != (dbi as u64).to_be_bytes() || value.len() > MAX_ROW
        || (dbi == 0 && key[8..15] != [0; 7]) { return Err(IO); }
    Ok(())
}
fn prepare<'a>(view: &native::KvRead<'a>, state: &State<'a>, dbi: u32) -> Result<Prepared<'a>> {
    let low = lower(dbi); let high = upper(dbi);
    let mut entries = 0usize; let mut values = 0usize;
    // Count first so allocation and its complete logical charge are admitted
    // before allocating. Both passes read the same immutable native snapshot.
    for next in view.cursor(Bound::Included(&low), Bound::Excluded(&high), false).map_err(code)? {
        let (key, value) = next.map_err(code)?;
        validate(dbi, key, value)?;
        entries = entries.checked_add(1).ok_or(IO)?;
        values = values.checked_add(value.len()).ok_or(IO)?;
        if state.entries.checked_add(entries).is_none_or(|n| n > state.limits.entries)
            || state.values.checked_add(values).is_none_or(|n| n > state.limits.values) { return Ok(Prepared::Unavailable); }
    }
    let slots = entries.max(1).checked_mul(2).and_then(usize::checked_next_power_of_two).ok_or(IO)?;
    let bytes = slots.checked_mul(size_of::<Option<View<'a>>>()).ok_or(IO)?;
    if state.bytes.checked_add(bytes).is_none_or(|n| n > state.limits.bytes) { return Ok(Prepared::Unavailable); }
    let mut table = Vec::new();
    if table.try_reserve_exact(slots).is_err() { return Ok(Prepared::Unavailable); }
    // Requested slot bytes are bounded; allocator bookkeeping/RSS are separate.
    table.resize(slots, None);
    let mut index = Index { slots: table.into_boxed_slice(), hash: RandomState::new() };
    let mut seen = 0usize; let mut seen_values = 0usize;
    for next in view.cursor(Bound::Included(&low), Bound::Excluded(&high), false).map_err(code)? {
        let (key, value) = next.map_err(code)?;
        validate(dbi, key, value)?;
        seen = seen.checked_add(1).ok_or(IO)?;
        seen_values = seen_values.checked_add(value.len()).ok_or(IO)?;
        if seen > entries || seen_values > values { return Err(IO); }
        if !index.insert(key, value)? { return Ok(Prepared::Unavailable); }
    }
    if seen != entries || seen_values != values { return Err(IO); }
    Ok(Prepared::Ready { index, entries, bytes, values })
}
impl<'a> Lookup<'a> {
    pub(super) fn new(read: &'a native::ReadTxn) -> Result<Self> {
        Ok(Self { view: read.kv(DATA).map_err(code)?,
            #[cfg(feature = "btree-blocks")]
            read,
            #[cfg(feature = "btree-blocks")]
            stores: RefCell::new(Vec::new()),
            #[cfg(all(feature = "btree-blocks", test))]
            audits: std::cell::Cell::new(0),
            state: RefCell::new(State {
            databases: Vec::new(), entries: 0, bytes: 0, values: 0, limits: LIMITS,
        }) })
    }
    pub(super) fn exact(&self, dbi: u32, key: &Key) -> Result<Option<Val>> {
        let mut state = self.state.borrow_mut();
        let at = match state.databases.binary_search_by_key(&dbi, |(dbi, _)| *dbi) {
            Ok(at) => at,
            Err(at) => {
                if state.databases.len() >= state.limits.databases
                    || state.bytes.checked_add(DB_CHARGE).is_none_or(|n| n > state.limits.bytes)
                    || state.databases.try_reserve_exact(1).is_err() {
                    return self.ordered(key);
                }
                state.databases.insert(at, (dbi, Database::Warming(0)));
                state.bytes += DB_CHARGE; at
            }
        };
        if let Database::Warming(requests) = &mut state.databases[at].1 {
            if *requests < REQUESTS { *requests += 1; return self.ordered(key); }
            // Poison first. An operational/corruption failure propagates, and
            // cannot leave a partially populated index visible on retry.
            state.databases[at].1 = Database::Ordered;
            #[cfg(not(feature = "btree-blocks"))]
            let preparation = prepare(&self.view, &state, dbi);
            #[cfg(feature = "btree-blocks")]
            let preparation = if dbi == 0 { prepare(&self.view, &state, dbi) } else { self.with_rows(dbi, |rows| prepare_blocks(rows, &state)) };
            match preparation {
                Ok(Prepared::Ready { index, entries, bytes, values }) => {
                    state.entries += entries; state.bytes += bytes; state.values += values;
                    state.databases[at].1 = Database::Indexed(index);
                }
                Ok(Prepared::Unavailable) => {}
                Err(error) => { state.databases[at].1 = Database::Failed(error); return Err(error); }
            }
        }
        match &state.databases[at].1 {
            Database::Failed(error) => Err(*error),
            Database::Indexed(index) => match index.get({
                #[cfg(feature = "btree-blocks")]
                { if dbi == 0 { key.as_slice() } else { &key[8..] } }
                #[cfg(not(feature = "btree-blocks"))]
                { key }
            }) {
                Probe::Value(value) => Ok(Some(borrowed(value))),
                Probe::Missing => Ok(None),
                Probe::Ordered => self.ordered(key),
            },
            _ => self.ordered(key),
        }
    }
    fn ordered(&self, key: &Key) -> Result<Option<Val>> {
        #[cfg(feature = "btree-blocks")]
        if let Some(dbi) = super::block_dbi(key)? { return self.with_rows(dbi, |rows| Ok(rows.exact(key[8..].try_into().unwrap()).map_err(super::block_code)?.map(|r| borrowed(r.value)))); }
        Ok(self.view.get(key).map_err(code)?.map(borrowed))
    }
    #[cfg(test)]
    pub(super) fn limits(&self, entries: usize, bytes: usize, values: usize, databases: usize) {
        let mut state = self.state.borrow_mut();
        assert!(state.databases.is_empty()); state.limits = Limits { entries, bytes, values, databases };
    }
    #[cfg(test)]
    pub(super) fn stats(&self) -> (usize, usize, usize, usize, usize) {
        let state = self.state.borrow();
        (state.entries, state.bytes, state.values, state.databases.len(),
            state.databases.iter().filter(|(_, db)| matches!(db, Database::Indexed(_))).count())
    }
}
fn borrowed(value: &[u8]) -> Val { Val { size: value.len(), data: value.as_ptr().cast_mut().cast() } }


#[cfg(feature = "btree-blocks")]
impl<'a> Lookup<'a> {
    fn with_rows<T>(&self, dbi:u32, action:impl FnOnce(&super::blocks::SnapshotRows<'a>)->Result<T>)->Result<T>{
        let mut stores=self.stores.borrow_mut();
        if let Ok(at)=stores.binary_search_by_key(&dbi,|entry|entry.0){return action(&stores[at].1)}
        #[cfg(test)]
        self.audits.set(self.audits.get() + 1);
        let rows=super::blocks::SnapshotRows::open_audited(self.read,DATA,dbi,super::blocks::Limits::default()).map_err(super::block_code)?;
        // One bounded directory per DBI on this exact actual ReadTxn. Refusal
        // leaves a local audited view, never a physical-record fallback.
        if stores.len()<1024&&stores.try_reserve_exact(1).is_ok(){let at=stores.binary_search_by_key(&dbi,|e|e.0).unwrap_err();stores.insert(at,(dbi,rows));action(&stores[at].1)}else{action(&rows)}
    }
    pub(super) fn rows_for<'b>(&self,read:&'b native::ReadTxn,dbi:u32)->Result<super::blocks::SnapshotRows<'b>> {
        self.with_rows(dbi,|rows| rows.for_read(read,DATA).map_err(super::block_code))
    }
    pub(super) fn cursor_for<'b>(&self,read:&'b native::ReadTxn,dbi:u32,lo:Bound<[u8;8]>,hi:Bound<[u8;8]>,reverse:bool)->Result<super::blocks::LogicalCursor<'b>> {
        self.with_rows(dbi,|rows| rows.cursor_for(read,DATA,lo,hi,reverse).map_err(super::block_code))
    }
    pub(super) fn value_for<'b>(&self,read:&'b native::ReadTxn,dbi:u32,key:&Key)->Result<Option<&'b[u8]>> {
        Ok(self.rows_for(read,dbi)?.exact(key[8..].try_into().unwrap()).map_err(super::block_code)?.map(|row|row.value))
    }
    #[cfg(test)]
    pub(super) fn block_audits(&self)->usize{self.audits.get()}
    pub(super) fn block_exact(&self,dbi:u32,key:&Key)->Result<Option<super::Row>>{
        self.with_rows(dbi,|rows|Ok(rows.exact(key[8..].try_into().unwrap()).map_err(super::block_code)?.map(super::block_row)))
    }
}
#[cfg(feature = "btree-blocks")]
fn prepare_blocks<'a>(rows:&super::blocks::SnapshotRows<'a>,state:&State<'a>)->Result<Prepared<'a>>{
    let mut entries=0usize;let mut values=0usize;
    for next in rows.cursor(Bound::Unbounded,Bound::Unbounded,false).map_err(super::block_code)?{let row=next.map_err(super::block_code)?;entries=entries.checked_add(1).ok_or(IO)?;values=values.checked_add(row.value.len()).ok_or(IO)?;
        if state.entries.checked_add(entries).is_none_or(|n|n>state.limits.entries)||state.values.checked_add(values).is_none_or(|n|n>state.limits.values){return Ok(Prepared::Unavailable)}}
    let slots=entries.max(1).checked_mul(2).and_then(usize::checked_next_power_of_two).ok_or(IO)?;let bytes=slots.checked_mul(size_of::<Option<View<'a>>>()).ok_or(IO)?;
    if state.bytes.checked_add(bytes).is_none_or(|n|n>state.limits.bytes){return Ok(Prepared::Unavailable)}let mut table=Vec::new();if table.try_reserve_exact(slots).is_err(){return Ok(Prepared::Unavailable)}table.resize(slots,None);let mut index=Index{slots:table.into_boxed_slice(),hash:RandomState::new()};
    let mut seen=0usize;let mut seen_values=0usize;
    for next in rows.cursor(Bound::Unbounded,Bound::Unbounded,false).map_err(super::block_code)?{let row=next.map_err(super::block_code)?;seen=seen.checked_add(1).ok_or(IO)?;seen_values=seen_values.checked_add(row.value.len()).ok_or(IO)?;if seen>entries||seen_values>values{return Err(IO)}if !index.insert(row.visible_key.as_slice(),row.value)?{return Ok(Prepared::Unavailable)}}
    if seen!=entries||seen_values!=values{return Err(IO)}Ok(Prepared::Ready{index,entries,bytes,values})
}
