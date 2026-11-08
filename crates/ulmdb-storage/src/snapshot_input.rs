//! Bounded owned staging bytes, copied into disjoint immutable chunk regions.
//!
//! A Slice owns its chunk and can outlive the Arena. Appends never overwrite an
//! exposed prefix. Reset is O(1): chunks are visited lazily in the next copy and
//! may be rewound only when Arc::get_mut proves exclusive ownership, including
//! synchronization with the final old owner. Pinned chunks can accept a fresh
//! suffix. At most four old chunks are probed before requesting a new chunk.
//!
//! The byte limit includes requested metadata-vector capacity, chunk payloads,
//! Chunk objects and Arc control headers. Allocator usable size/RSS and the
//! Arena object itself are separate. Refusal leaves all existing slices valid.
//! Published tree values must copy into their separately charged immutable
//! storage rather than retaining staging slices as admitted tree ownership.
use crate::{Error, Result};
use std::{cell::UnsafeCell, mem::size_of, sync::Arc};

const CHUNK_BYTES: usize = 4096;
const MAX_PROBES: usize = 4;
const ARC_HEADER: usize = 2 * size_of::<usize>();
struct Chunk {
    bytes: Box<[UnsafeCell<u8>]>,
}

// Shared references expose only sealed Slice regions. Arena is the only writer
// and writes a disjoint unexposed suffix, or reuses after Arc::get_mut succeeds.
#[allow(unsafe_code)]
mod cells {
    use super::*;
    // SAFETY: exposed regions remain immutable for every owning Slice's entire
    // lifetime. Bytes outside those regions can be initialized concurrently;
    // UnsafeCell permits those disjoint writes without freezing the whole box.
    unsafe impl Sync for Chunk {}
    pub(super) fn copy(chunk: &Chunk, start: usize, input: &[u8]) {
        debug_assert!(
            start
                .checked_add(input.len())
                .is_some_and(|n| n <= chunk.bytes.len())
        );
        // SAFETY: the Arena reserves this never-exposed destination interval.
        // A source Slice keeps its Arc alive, preventing any rewind of its
        // source chunk; its sealed prefix cannot overlap the fresh suffix.
        unsafe {
            let destination = chunk.bytes.as_ptr().add(start).cast::<u8>().cast_mut();
            std::ptr::copy_nonoverlapping(input.as_ptr(), destination, input.len());
        }
    }
    pub(super) fn borrow(chunk: &Chunk, start: usize, length: usize) -> &[u8] {
        debug_assert!(
            start
                .checked_add(length)
                .is_some_and(|n| n <= chunk.bytes.len())
        );
        // SAFETY: Slice owns this exact chunk and was created only after the
        // interval was initialized/sealed. Its Arc prevents rewind/reuse. This
        // reference covers only that interval, not concurrently appended bytes.
        unsafe { std::slice::from_raw_parts(chunk.bytes.as_ptr().add(start).cast::<u8>(), length) }
    }
}

/// Owned immutable staging bytes. One Arc reference and two 32-bit extents;
/// cloning allocates nothing. This fits within16 bytes on supported targets.
#[derive(Clone)]
pub struct Slice {
    chunk: Arc<Chunk>,
    start: u32,
    length: u32,
}
const _: () = assert!(size_of::<Slice>() <= 16);
impl Slice {
    #[inline]
    pub fn as_slice(&self) -> &[u8] {
        cells::borrow(&self.chunk, self.start as usize, self.length as usize)
    }
}
impl std::fmt::Debug for Slice {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_tuple("Slice").field(&self.as_slice()).finish()
    }
}
struct Entry {
    chunk: Arc<Chunk>,
    used: u32,
    generation: u64,
}
#[derive(Clone, Copy, Debug, Default)]
pub struct Stats {
    pub limit_bytes: usize,
    pub allocated_bytes: usize,
    pub peak_bytes: usize,
    pub chunks: usize,
    pub metadata_bytes: usize,
    pub payload_bytes: usize,
    pub generation: u64,
    pub exhausted: bool,
}
pub struct Arena {
    limit: usize,
    allocated: usize,
    peak: usize,
    metadata_capacity: usize,
    payload_bytes: usize,
    chunks: Vec<Entry>,
    generation: u64,
    anchor: usize,
    cursor: usize,
    last: usize,
    exhausted: bool,
}
impl Arena {
    /// Empty and allocation-free. copy refuses when this limit cannot cover
    /// its requested chunk/control/metadata allocations, including zero limits.
    pub fn new(limit_bytes: usize) -> Self {
        Self {
            limit: limit_bytes,
            allocated: 0,
            peak: 0,
            metadata_capacity: 0,
            payload_bytes: 0,
            chunks: Vec::new(),
            generation: 1,
            anchor: 0,
            cursor: 0,
            last: 0,
            exhausted: false,
        }
    }
    /// Constant work regardless of seed size. No slice is invalidated. Starting
    /// near the last successful chunk avoids rescanning a large pinned prefix.
    pub fn reset(&mut self) {
        if let Some(next) = self.generation.checked_add(1) {
            self.generation = next;
            self.anchor = self.last;
            self.cursor = 0;
        } else {
            self.exhausted = true;
        }
    }
    pub fn stats(&self) -> Stats {
        Stats {
            limit_bytes: self.limit,
            allocated_bytes: self.allocated,
            peak_bytes: self.peak,
            chunks: self.chunks.len(),
            metadata_bytes: self.metadata_capacity * size_of::<Entry>(),
            payload_bytes: self.payload_bytes,
            generation: self.generation,
            exhausted: self.exhausted,
        }
    }
    fn charge(&mut self, bytes: usize) {
        self.allocated += bytes;
        self.peak = self.peak.max(self.allocated);
    }
    fn allocate(&mut self, length: usize) -> Result<usize> {
        let payload = CHUNK_BYTES.max(length);
        let capacity = if self.chunks.len() == self.metadata_capacity {
            self.metadata_capacity
                .max(1)
                .checked_mul(2)
                .ok_or(Error::LimitExceeded)?
        } else {
            self.metadata_capacity
        };
        let metadata = (capacity - self.metadata_capacity)
            .checked_mul(size_of::<Entry>())
            .ok_or(Error::LimitExceeded)?;
        let chunk = payload
            .checked_add(size_of::<Chunk>() + ARC_HEADER)
            .ok_or(Error::LimitExceeded)?;
        if self
            .allocated
            .checked_add(metadata)
            .and_then(|n| n.checked_add(chunk))
            .is_none_or(|n| n > self.limit)
        {
            return Err(Error::LimitExceeded);
        }
        if metadata != 0 {
            self.chunks
                .try_reserve_exact(capacity - self.chunks.len())
                .map_err(|_| Error::LimitExceeded)?;
            self.metadata_capacity = capacity;
            self.charge(metadata);
        }
        let mut bytes = Vec::new();
        bytes
            .try_reserve_exact(payload)
            .map_err(|_| Error::LimitExceeded)?;
        for _ in 0..payload {
            bytes.push(UnsafeCell::new(0));
        }
        let index = self.chunks.len();
        self.chunks.push(Entry {
            chunk: Arc::new(Chunk {
                bytes: bytes.into_boxed_slice(),
            }),
            used: 0,
            generation: self.generation,
        });
        self.payload_bytes += payload;
        self.charge(chunk);
        Ok(index)
    }
    pub fn copy(&mut self, bytes: &[u8]) -> Result<Slice> {
        if self.exhausted || bytes.len() > u32::MAX as usize {
            return Err(Error::LimitExceeded);
        }
        let length = bytes.len() as u32;
        let count = self.chunks.len();
        for _ in 0..MAX_PROBES.min(count.saturating_sub(self.cursor)) {
            let index = (self.anchor + self.cursor) % count;
            let entry = &mut self.chunks[index];
            if entry.generation != self.generation {
                if Arc::get_mut(&mut entry.chunk).is_some() {
                    entry.used = 0;
                }
                entry.generation = self.generation;
            }
            if let Some(end) = entry
                .used
                .checked_add(length)
                .filter(|end| *end as usize <= entry.chunk.bytes.len())
            {
                let start = entry.used;
                cells::copy(&entry.chunk, start as usize, bytes);
                entry.used = end;
                self.last = index;
                return Ok(Slice {
                    chunk: entry.chunk.clone(),
                    start,
                    length,
                });
            }
            self.cursor += 1;
        }
        let index = self.allocate(bytes.len())?;
        self.anchor = index;
        self.cursor = 0;
        self.last = index;
        let entry = &mut self.chunks[index];
        cells::copy(&entry.chunk, 0, bytes);
        entry.used = length;
        Ok(Slice {
            chunk: entry.chunk.clone(),
            start: 0,
            length,
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn slices_share_one_chunk_and_outlive_arena_and_reset() {
        assert_eq!(size_of::<Slice>(), 16);
        let mut arena = Arena::new(64 << 10);
        let first = arena.copy(b"first").unwrap();
        let second = arena.copy(b"second").unwrap();
        assert!(Arc::ptr_eq(&first.chunk, &second.chunk));
        assert_eq!(arena.stats().chunks, 1);
        arena.reset();
        let third = arena.copy(b"third").unwrap();
        assert!(Arc::ptr_eq(&first.chunk, &third.chunk));
        assert_eq!(first.as_slice(), b"first");
        assert_eq!(second.as_slice(), b"second");
        assert_eq!(third.as_slice(), b"third");
        drop(arena);
        assert_eq!(first.as_slice(), b"first");
        assert_eq!(third.clone().as_slice(), b"third");
    }
    #[test]
    fn unique_chunk_rewinds_after_reset_without_new_allocation() {
        let mut arena = Arena::new(16 << 10);
        let first = arena.copy(&[1; 4096]).unwrap();
        let address = first.as_slice().as_ptr();
        drop(first);
        let charged = arena.stats().allocated_bytes;
        arena.reset();
        let next = arena.copy(&[2; 4096]).unwrap();
        assert_eq!(next.as_slice().as_ptr(), address);
        assert_eq!(next.as_slice(), &[2; 4096]);
        assert_eq!(arena.stats().allocated_bytes, charged);
    }
    #[test]
    fn exact_budget_refusal_and_generation_exhaustion_preserve_old_slices() {
        let cost = 2 * size_of::<Entry>() + CHUNK_BYTES + size_of::<Chunk>() + ARC_HEADER;
        let mut arena = Arena::new(cost);
        let old = arena.copy(&[3; 4096]).unwrap();
        assert_eq!(arena.stats().allocated_bytes, cost);
        assert!(matches!(arena.copy(b"no room"), Err(Error::LimitExceeded)));
        assert_eq!(old.as_slice(), &[3; 4096]);
        drop(old);
        arena.reset();
        let old = arena.copy(&[4; 4096]).unwrap();
        arena.generation = u64::MAX;
        arena.reset();
        assert!(arena.stats().exhausted);
        assert!(matches!(
            arena.copy(b"exhausted"),
            Err(Error::LimitExceeded)
        ));
        assert_eq!(old.as_slice(), &[4; 4096]);
        assert!(matches!(
            Arena::new(0).copy(b"x"),
            Err(Error::LimitExceeded)
        ));
    }
    #[test]
    fn clone_pins_full_chunk_and_partial_chunks_append_without_overwrite() {
        let mut arena = Arena::new(32 << 10);
        let first = arena.copy(&[5; 2048]).unwrap();
        let clone = first.clone();
        drop(first);
        arena.reset();
        let tail = arena.copy(&[6; 2048]).unwrap();
        assert!(Arc::ptr_eq(&clone.chunk, &tail.chunk));
        assert_ne!(clone.as_slice().as_ptr(), tail.as_slice().as_ptr());
        arena.reset();
        let next = arena.copy(&[7; 4096]).unwrap();
        assert!(!Arc::ptr_eq(&clone.chunk, &next.chunk));
        assert_eq!(clone.as_slice(), &[5; 2048]);
        assert_eq!(tail.as_slice(), &[6; 2048]);
    }
    #[test]
    fn concurrent_slice_read_is_stable_during_append_and_many_resets() {
        use std::sync::mpsc;
        let mut arena = Arena::new(256 << 10);
        let held = arena.copy(&[8; 2048]).unwrap();
        std::thread::scope(|scope| {
            let (ready_tx, ready_rx) = mpsc::channel();
            let (done_tx, done_rx) = mpsc::channel();
            let reader = scope.spawn(move || {
                let bytes = held.as_slice();
                let address = bytes.as_ptr();
                ready_tx.send(()).unwrap();
                while done_rx.try_recv().is_err() {
                    assert_eq!(bytes, &[8; 2048]);
                    assert_eq!(held.as_slice().as_ptr(), address);
                    std::hint::spin_loop();
                }
                assert_eq!(bytes, &[8; 2048]);
            });
            ready_rx.recv().unwrap();
            for _ in 0..1000 {
                arena.reset();
                let next = arena.copy(&[9; 64]).unwrap();
                assert_eq!(next.as_slice(), &[9; 64]);
            }
            done_tx.send(()).unwrap();
            reader.join().unwrap();
        });
    }
    #[test]
    fn reset_after_large_seed_starts_near_last_chunk_and_large_and_empty_values_work() {
        let mut arena = Arena::new(1 << 20);
        let mut pinned = Vec::new();
        for _ in 0..100 {
            pinned.push(arena.copy(&[10; 4096]).unwrap());
        }
        let last = pinned.pop().unwrap();
        let last_address = last.as_slice().as_ptr();
        drop(last);
        let chunks = arena.stats().chunks;
        for _ in 0..1000 {
            arena.reset();
            let next = arena.copy(b"small").unwrap();
            assert_eq!(next.as_slice().as_ptr(), last_address);
            assert_eq!(arena.stats().chunks, chunks);
        }
        let large = arena.copy(&[11; 8193]).unwrap();
        assert_eq!(large.as_slice(), &[11; 8193]);
        let empty = arena.copy(b"").unwrap();
        assert!(empty.as_slice().is_empty());
        for slice in pinned {
            assert_eq!(slice.as_slice(), &[10; 4096]);
        }
        assert!(arena.stats().allocated_bytes <= arena.stats().limit_bytes);
    }
    #[test]
    fn copying_a_slice_back_into_arena_keeps_source_and_destination_disjoint() {
        let mut arena = Arena::new(16 << 10);
        let source = arena.copy(&[12; 1024]).unwrap();
        arena.reset();
        let duplicate = arena.copy(source.as_slice()).unwrap();
        assert!(Arc::ptr_eq(&source.chunk, &duplicate.chunk));
        assert_ne!(source.as_slice().as_ptr(), duplicate.as_slice().as_ptr());
        assert_eq!(duplicate.as_slice(), source.as_slice());
    }
}
