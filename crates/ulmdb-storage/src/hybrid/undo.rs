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
pub struct ImageCheckpoint {
    owner: Arc<()>,
    id: u64,
}

pub(super) struct Frame {
    id: u64,
    pub length: usize,
    pub revision: u64,
    pub boundary_object: Option<super::arena::Object>,
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
        boundary_object: Option<super::arena::Object>,
    ) -> Result<ImageCheckpoint> {
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
        Ok(ImageCheckpoint {
            owner: Arc::clone(&self.owner),
            id,
        })
    }
    fn position(&self, token: &ImageCheckpoint) -> Result<usize> {
        if !Arc::ptr_eq(&self.owner, &token.owner) {
            return Err(Error::InvalidInput("checkpoint belongs to another arena"));
        }
        self.frames
            .iter()
            .position(|frame| frame.id == token.id)
            .ok_or(Error::InvalidInput("inactive checkpoint"))
    }
    pub fn release(&mut self, token: &ImageCheckpoint) -> Result<()> {
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
    pub fn rollback(&mut self, token: &ImageCheckpoint) -> Result<Frame> {
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
    pub fn remember(&mut self, start: usize, source: &[u8]) -> Result<()> {
        let length = source.len();
        let end = start.checked_add(length).ok_or(Error::LimitExceeded)?;
        let needed = self
            .frames
            .iter()
            .filter(|f| start < f.length && !f.before.contains_key(&start))
            .count();
        let total = self
            .bytes
            .checked_add(needed.checked_mul(length).ok_or(Error::LimitExceeded)?)
            .ok_or(Error::LimitExceeded)?;
        if total > MAX_UNDO_BYTES {
            return Err(Error::LimitExceeded);
        }
        if self
            .frames
            .iter()
            .any(|f| start < f.length && !f.before.contains_key(&start) && end > f.length)
        {
            return Err(Error::Corrupt("image object crosses checkpoint"));
        }
        for f in &mut self.frames {
            if start < f.length && !f.before.contains_key(&start) {
                f.before.insert(start, source.to_vec());
                f.undo_bytes += length;
            }
        }
        self.bytes = total;
        Ok(())
    }
}
