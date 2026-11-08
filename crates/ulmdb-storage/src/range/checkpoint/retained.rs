//! Fixed, backend-typed retained records anchored to a genuine checkpoint.
use super::*;
use sha2::{Digest, Sha256};
const BYTES: usize = 256;
const MAGIC: &[u8; 8] = b"ULMDBRP1";
fn write_receipt(bytes: &mut [u8], receipt: pack::Receipt) {
    bytes[..8].copy_from_slice(&receipt.offset.to_le_bytes());
    bytes[8..16].copy_from_slice(&receipt.ordinal.to_le_bytes());
    bytes[16..24].copy_from_slice(&receipt.payload_length.to_le_bytes());
    bytes[24..56].copy_from_slice(&receipt.hash);
}
fn read_receipt(bytes: &[u8]) -> Result<pack::Receipt> {
    let receipt = pack::Receipt {
        offset: u64::from_le_bytes(bytes[..8].try_into().unwrap()),
        ordinal: u64::from_le_bytes(bytes[8..16].try_into().unwrap()),
        payload_length: u64::from_le_bytes(bytes[16..24].try_into().unwrap()),
        hash: bytes[24..56].try_into().unwrap(),
    };
    receipt.validate()?;
    Ok(receipt)
}
impl RangeCheckpoint {
    fn check_same_asset(&self, other: &Self) -> Result<()> {
        self.check_creator()?;
        other.check_creator()?;
        if self.inner.file != other.inner.file
            || self.inner.uuid != other.inner.uuid
            || self.inner.epoch != other.inner.epoch
            || !ulmdb_platform::file_identity(&self.inner.source)?
                .same_object(&ulmdb_platform::file_identity(&other.inner.source)?)
            || !self
                .inner
                .namespace
                .as_ref()
                .unwrap()
                .same_binding(other.inner.namespace.as_ref().unwrap())?
        {
            return Err(Error::InvalidInput("foreign retained range checkpoint"));
        }
        Ok(())
    }
    // Every step authenticates the receipt from its authenticated child. Raw
    // pin bytes cannot name a valid-looking unpublished frame inside user data.
    fn check_ancestor(&self, selected: pack::Receipt) -> Result<()> {
        self.check_creator()?;
        if selected.ordinal > self.inner.selected.ordinal
            || selected.frame_end()? > self.inner.selected.frame_end()?
        {
            return Err(Error::Corrupt(
                "retained range checkpoint is newer than anchor",
            ));
        }
        let mut at = self.inner.selected;
        let mut remaining = at.ordinal - selected.ordinal;
        while remaining != 0 {
            let frame = pack::recovery::verify_envelope(&self.inner.source, self.inner.file, at)?;
            at = frame
                .parent
                .ok_or(Error::Corrupt("retained range ancestry ended early"))?;
            remaining -= 1;
        }
        if at != selected {
            return Err(Error::Corrupt(
                "retained range checkpoint is not an ancestor",
            ));
        }
        Ok(())
    }
    /// Serialize this genuine historical checkpoint only under a genuine same-
    /// namespace descendant. This is a portable checked record, not a file pin.
    pub fn retained_record(&self, current: &Self) -> Result<[u8; BYTES]> {
        let anchor = current.cross_asset_head(self.inner.file)?;
        self.check_same_asset(&anchor)?;
        anchor.check_ancestor(self.inner.selected)?;
        pack::recovery::verify_frame(&self.inner.source, self.inner.file, self.inner.selected)?;
        let mut b = [0; BYTES];
        b[..8].copy_from_slice(MAGIC);
        b[8..10].copy_from_slice(&1u16.to_le_bytes());
        b[12..16].copy_from_slice(&(BYTES as u32).to_le_bytes());
        b[16..32].copy_from_slice(&self.inner.file);
        b[32..48].copy_from_slice(&self.inner.uuid);
        write_receipt(&mut b[48..104], self.inner.selected);
        if let Some(parent) = self.inner.parent {
            write_receipt(&mut b[104..160], parent);
        }
        b[160..176].copy_from_slice(&self.inner.generation.to_le_bytes());
        b[176..184].copy_from_slice(&self.inner.sequence.to_le_bytes());
        b[184..192].copy_from_slice(&self.inner.epoch.to_le_bytes());
        b[192..224].copy_from_slice(&self.inner.scalar);
        let hash = Sha256::digest(&b[..224]);
        b[224..].copy_from_slice(&hash);
        Ok(b)
    }
    /// Decode an untrusted retained record under this authentic selected anchor.
    /// No CURRENT/head rediscovery, healing, truncation or publication occurs.
    pub fn open_retained_checked<F: FnOnce(&SegmentTable, &Manifest) -> Result<()>>(
        &self,
        bytes: &[u8],
        controller: &ResidencyController,
        validate: F,
    ) -> Result<RangeSnapshot> {
        self.check_creator()?;
        if bytes.len() == BYTES && bytes[16..32] != self.inner.file {
            let head = self.cross_asset_head(bytes[16..32].try_into().unwrap())?;
            return head.open_retained_checked(bytes, controller, validate);
        }
        if bytes.len() != BYTES
            || &bytes[..8] != MAGIC
            || bytes[8..10] != 1u16.to_le_bytes()
            || bytes[10..12] != [0; 2]
            || bytes[12..16] != (BYTES as u32).to_le_bytes()
            || <[u8; 32]>::from(Sha256::digest(&bytes[..224])) != bytes[224..]
            || bytes[16..32] != self.inner.file
            || bytes[32..48] != self.inner.uuid
            || bytes[184..192] != self.inner.epoch.to_le_bytes()
        {
            return Err(Error::Corrupt("retained range record header/realm/hash"));
        }
        let selected = read_receipt(&bytes[48..104])?;
        let parent = if bytes[104..160] == [0; 56] {
            None
        } else {
            Some(read_receipt(&bytes[104..160])?)
        };
        self.check_ancestor(selected)?;
        let checkpoint = Self {
            inner: Arc::new(CheckpointInner {
                source: self.inner.source.clone(),
                creator: self.inner.creator,
                namespace: self.inner.namespace.clone(),
                epoch: self.inner.epoch,
                file: self.inner.file,
                uuid: self.inner.uuid,
                selected,
                parent,
                generation: u128::from_le_bytes(bytes[160..176].try_into().unwrap()),
                sequence: u64::from_le_bytes(bytes[176..184].try_into().unwrap()),
                scalar: bytes[192..224].try_into().unwrap(),
            }),
        };
        checkpoint.load(controller, validate)
    }
}

impl RangeCheckpoint {
    pub(in crate::range) fn matches_physical(
        &self,
        file: [u8; 16],
        receipt: pack::Receipt,
        scalar: [u8; 32],
    ) -> bool {
        self.check_creator().is_ok()
            && self.inner.file == file
            && self.inner.selected == receipt
            && self.inner.scalar == scalar
    }
    pub(in crate::range) fn rotation_anchor(
        &self,
        candidate: &Manifest,
    ) -> Result<super::super::rotation::Anchor> {
        self.check_creator()?;
        let mut header = [0; 4096];
        pack::io::read_exact_at(
            &*self.inner.source,
            &mut header,
            self.inner.selected.offset + 4096,
        )?;
        pack::recovery::verify_frame(&self.inner.source, self.inner.file, self.inner.selected)?;
        if header[16..32] != candidate.uuid
            || header[48..56] != candidate.epoch.to_le_bytes()
            || header[56..64] != candidate.sequence.to_le_bytes()
            || header[64..80] != candidate.commit.to_le_bytes()
            || header[80..96] != candidate.parent.to_le_bytes()
            || header[176..184] != candidate.next_database.to_le_bytes()
            || candidate.page_size != ulmdb_core::PageSize::DEFAULT
            || candidate.value_limit != ulmdb_core::ValueLimit::DEFAULT
        {
            return Err(Error::Corrupt(
                "range rotation changed logical identity/profile",
            ));
        }
        Ok(super::super::rotation::Anchor {
            source: self.retained_record(self)?,
            source_global: Sha256::digest(super::super::rotation::global_bytes(
                &self.inner.source,
            )?)
            .into(),
            commit: candidate.commit,
            parent: candidate.parent,
            next_database: candidate.next_database,
        })
    }
    // Follow authenticated global certificates, without requiring intermediate
    // data packs to survive. A floor cannot advance past a live/persisted pin.
    fn cross_asset_head(&self, target: [u8; 16]) -> Result<Self> {
        self.check_creator()?;
        if target == self.inner.file {
            return Ok(self.clone());
        }
        let namespace = self.inner.namespace.as_ref().unwrap();
        let directory = namespace.path();
        if !namespace.matches(directory)? {
            return Err(Error::StaleHandle);
        }
        let selected = super::super::rotation::selection(directory)?;
        if selected.uuid != self.inner.uuid {
            return Err(Error::StaleHandle);
        }
        let mut at = self.inner.file;
        let mut global = super::super::rotation::global(&self.inner.source)?;
        let mut seen = std::collections::BTreeSet::new();
        for _ in 0..4096 {
            if at == selected.floor || !seen.insert(at) {
                return Err(Error::Corrupt(
                    "retained range outside floor/cyclic lineage",
                ));
            }
            let anchor = global
                .anchor
                .ok_or(Error::Corrupt("retained range missing rotation anchor"))?;
            if anchor.source[32..48] != self.inner.uuid
                || anchor.source[184..192] != self.inner.epoch.to_le_bytes()
            {
                return Err(Error::Corrupt("range anchor realm"));
            }
            at = anchor.file();
            global = super::super::rotation::archived_global(directory, at, anchor.source_global)?;
            if at == target {
                let reader = Arc::new(namespace.reader_asset(directory, target)?);
                let bytes = anchor.source;
                let selected = read_receipt(&bytes[48..104])?;
                let parent = if bytes[104..160] == [0; 56] {
                    None
                } else {
                    Some(read_receipt(&bytes[104..160])?)
                };
                return Ok(Self {
                    inner: Arc::new(CheckpointInner {
                        source: reader,
                        creator: self.inner.creator,
                        namespace: Some(namespace.clone()),
                        epoch: self.inner.epoch,
                        file: at,
                        uuid: self.inner.uuid,
                        selected,
                        parent,
                        generation: u128::from_le_bytes(bytes[160..176].try_into().unwrap()),
                        sequence: u64::from_le_bytes(bytes[176..184].try_into().unwrap()),
                        scalar: bytes[192..224].try_into().unwrap(),
                    }),
                });
            }
        }
        Err(Error::LimitExceeded)
    }
}
