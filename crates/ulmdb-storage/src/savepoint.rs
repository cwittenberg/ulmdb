//! Writer-private undo journal. One before-image per object/checkpoint, captured
//! only when that object predates the checkpoint. New allocation tails truncate.
use crate::{Error, Result};
use std::{collections::BTreeMap, sync::Arc};

pub(super) const MAX_DEPTH: usize = 64;
pub(super) const MAX_UNDO_BYTES: usize = 268_435_456;

/// Opaque arena-specific checkpoint. Checkpoints cannot be transferred between
/// arenas; releasing requires the newest active checkpoint. Rolling back an
/// older checkpoint also discards its descendants.
#[derive(Debug)]
pub struct ArenaCheckpoint {
    owner: Arc<()>,
    id: u64,
}

pub(super) struct Frame {
    id: u64,
    pub length: usize,
    pub revision: u64,
    pub boundary_object: Option<usize>,
    pub before: BTreeMap<usize, Vec<u8>>,
    undo_bytes: usize,
}

pub(super) struct Journal {
    owner: Arc<()>,
    next: u64,
    frames: Vec<Frame>,
    bytes: usize,
}
impl Journal {
    pub fn new() -> Self {
        Self {
            owner: Arc::new(()),
            next: 1,
            frames: Vec::new(),
            bytes: 0,
        }
    }
    pub fn depth(&self) -> usize {
        self.frames.len()
    }
    pub fn bytes(&self) -> usize {
        self.bytes
    }
    pub fn checkpoint(
        &mut self,
        length: usize,
        revision: u64,
        boundary_object: Option<usize>,
    ) -> Result<ArenaCheckpoint> {
        if self.frames.len() >= MAX_DEPTH {
            return Err(Error::LimitExceeded);
        }
        let next = self.next.checked_add(1).ok_or(Error::LimitExceeded)?;
        let id = self.next;
        self.frames.push(Frame {
            id,
            length,
            revision,
            boundary_object,
            before: BTreeMap::new(),
            undo_bytes: 0,
        });
        self.next = next;
        Ok(ArenaCheckpoint {
            owner: Arc::clone(&self.owner),
            id,
        })
    }
    fn position(&self, token: &ArenaCheckpoint) -> Result<usize> {
        if !Arc::ptr_eq(&self.owner, &token.owner) {
            return Err(Error::InvalidInput("checkpoint belongs to another arena"));
        }
        self.frames
            .iter()
            .position(|frame| frame.id == token.id)
            .ok_or(Error::InvalidInput("inactive checkpoint"))
    }
    pub fn release(&mut self, token: &ArenaCheckpoint) -> Result<()> {
        let index = self.position(token)?;
        if index + 1 != self.frames.len() {
            return Err(Error::InvalidInput("release newest checkpoint first"));
        }
        let frame = self
            .frames
            .pop()
            .ok_or(Error::InvalidInput("inactive checkpoint"))?;
        self.bytes -= frame.undo_bytes;
        Ok(())
    }
    pub fn rollback(&mut self, token: &ArenaCheckpoint) -> Result<Frame> {
        let index = self.position(token)?;
        while self.frames.len() > index + 1 {
            let frame = self
                .frames
                .pop()
                .ok_or(Error::InvalidInput("inactive checkpoint"))?;
            self.bytes -= frame.undo_bytes;
        }
        let frame = self
            .frames
            .pop()
            .ok_or(Error::InvalidInput("inactive checkpoint"))?;
        self.bytes -= frame.undo_bytes;
        Ok(frame)
    }
    #[cfg(test)]
    pub fn remember(&mut self, bytes: &[u8], start: usize, length: usize) -> Result<()> {
        self.remember_with_limit(bytes, start, length, MAX_UNDO_BYTES)
    }
    pub fn remember_with_limit(
        &mut self,
        bytes: &[u8],
        start: usize,
        length: usize,
        limit: usize,
    ) -> Result<()> {
        let end = start.checked_add(length).ok_or(Error::LimitExceeded)?;
        let source = bytes
            .get(start..end)
            .ok_or(Error::Corrupt("undo source outside arena"))?;
        let needed = self
            .frames
            .iter()
            .filter(|frame| start < frame.length && !frame.before.contains_key(&start))
            .count();
        let addition = needed.checked_mul(length).ok_or(Error::LimitExceeded)?;
        let total = self
            .bytes
            .checked_add(addition)
            .ok_or(Error::LimitExceeded)?;
        if total > limit {
            return Err(Error::LimitExceeded);
        }
        if self.frames.iter().any(|frame| {
            start < frame.length && !frame.before.contains_key(&start) && end > frame.length
        }) {
            return Err(Error::Corrupt("object crosses checkpoint boundary"));
        }
        for frame in &mut self.frames {
            if start < frame.length && !frame.before.contains_key(&start) {
                frame.before.insert(start, source.to_vec());
                frame.undo_bytes += length;
            }
        }
        self.bytes = total;
        Ok(())
    }
}
