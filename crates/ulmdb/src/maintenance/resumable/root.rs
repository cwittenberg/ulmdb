//! Owned authenticated metadata shared by discovery and marking, without flattening.
use super::*;
use ulmdb_storage::{DescriptorFootprint, IndexedDescriptorFootprint};

pub(super) enum ProtectedRoot {
    Manifest(Manifest),
    Flat(DescriptorFootprint),
    Indexed(IndexedDescriptorFootprint),
}
impl ProtectedRoot {
    pub fn uuid(&self) -> [u8; 16] {
        match self {
            Self::Manifest(m) => m.uuid,
            Self::Flat(m) => m.header().uuid,
            Self::Indexed(m) => m.header().uuid,
        }
    }
    pub fn epoch(&self) -> u64 {
        match self {
            Self::Manifest(m) => m.epoch,
            Self::Flat(m) => m.header().epoch,
            Self::Indexed(m) => m.header().epoch,
        }
    }
    pub fn generation(&self) -> u128 {
        match self {
            Self::Manifest(m) => m.generation,
            Self::Flat(m) => m.header().generation,
            Self::Indexed(m) => m.header().generation,
        }
    }
    pub fn sequence(&self) -> u64 {
        match self {
            Self::Manifest(m) => m.sequence,
            Self::Flat(m) => m.header().sequence,
            Self::Indexed(m) => m.header().sequence,
        }
    }
    pub fn commit(&self) -> u128 {
        match self {
            Self::Manifest(m) => m.commit,
            Self::Flat(m) => m.header().commit,
            Self::Indexed(m) => m.header().commit,
        }
    }
    pub fn minor(&self) -> u16 {
        match self {
            Self::Manifest(m) => m.format_minor,
            Self::Flat(m) => m.header().format_minor,
            Self::Indexed(m) => m.header().format_minor,
        }
    }
    pub fn len(&self) -> usize {
        match self {
            Self::Manifest(m) => m.segments.len(),
            Self::Flat(m) => m.header().segment_count,
            Self::Indexed(m) => m.header().segment_count,
        }
    }
    pub fn entry(&self, at: usize) -> Option<&SegmentEntry> {
        match self {
            Self::Manifest(m) => m.segments.get(at),
            Self::Flat(m) => m.entry(at),
            Self::Indexed(m) => m.entry(at),
        }
    }
    pub fn has_hash(&self, hash: &Hash) -> bool {
        (0..self.len()).any(|at| self.entry(at).is_some_and(|e| &e.hash == hash))
    }
    pub fn has_cohort(&self, generation: u128) -> bool {
        self.generation() == generation
            || (0..self.len()).any(|at| self.entry(at).is_some_and(|e| e.cohort == generation))
    }
}
impl ulmdb_storage::fixed_map::FixedSlotRoot for ProtectedRoot {
    fn uuid(&self) -> [u8; 16] {
        self.uuid()
    }
    fn epoch(&self) -> u64 {
        self.epoch()
    }
    fn inventory_len(&self) -> usize {
        self.len()
    }
    fn inventory_entry(&self, at: usize) -> Option<&SegmentEntry> {
        self.entry(at)
    }
}
