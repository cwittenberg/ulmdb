use crate::hybrid::redo::{RootState, WalReceipt};
use crate::{Error, Reference, Result};
use sha2::{Digest, Sha256};
/// Fixed startup prefix only; records remain densely 8-byte aligned.
pub const DATA_START: u64 = 196608;
/// Separate declared 64KiB failure granules; no atomic-write assumption.
pub const WITNESS_OFFSETS: [u64; 2] = [65536, 131072];
pub const BLOCK: usize = 256;
pub const MAX_WAL_BYTES: u64 = 268_435_456;
/// Explicit on-disk WAL geometry. Capacity never grants record authority.
#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub enum WalLayout {
    #[default]
    Dense,
    PreSized {
        capacity: u64,
    },
}
impl WalLayout {
    pub fn validate(self) -> Result<()> {
        if let Self::PreSized { capacity } = self
            && (!(DATA_START..=MAX_WAL_BYTES).contains(&capacity) || !capacity.is_multiple_of(4096))
        {
            return Err(Error::InvalidInput("hybrid pre-sized WAL capacity"));
        }
        Ok(())
    }
    pub fn physical_end(self, logical_end: u64) -> u64 {
        match self {
            Self::Dense => logical_end,
            Self::PreSized { capacity } => capacity,
        }
    }
    pub fn limit(self) -> u64 {
        self.physical_end(MAX_WAL_BYTES)
    }
}
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct Bootstrap {
    pub uuid: [u8; 16],
    pub epoch: u64,
    pub commit: u128,
}
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct Witness {
    pub receipt: WalReceipt,
    pub commit: u128,
}
fn hash(domain: &[u8], bytes: &[u8]) -> [u8; 32] {
    let mut h = Sha256::new();
    h.update(domain);
    h.update(bytes);
    h.finalize().into()
}
fn put64(b: &mut [u8], at: usize, n: u64) {
    b[at..at + 8].copy_from_slice(&ulmdb_core::endian::encode_u64_le(n));
}
fn get64(b: &[u8], at: usize) -> u64 {
    ulmdb_core::endian::decode_u64_le(b[at..at + 8].try_into().unwrap())
}
fn seal(b: &mut [u8; BLOCK], domain: &[u8]) {
    let h = hash(domain, &b[..224]);
    b[224..].copy_from_slice(&h);
}
pub fn encode_header(h: Bootstrap) -> Result<[u8; BLOCK]> {
    encode_layout_header(h, WalLayout::Dense)
}
pub fn encode_layout_header(h: Bootstrap, layout: WalLayout) -> Result<[u8; BLOCK]> {
    layout.validate()?;
    if h.uuid == [0; 16] || h.epoch == 0 || h.commit == 0 {
        return Err(Error::InvalidInput("hybrid bootstrap identity"));
    }
    let mut b = [0; BLOCK];
    b[..8].copy_from_slice(b"ULMDBHW1");
    b[8..12].copy_from_slice(&1u32.to_le_bytes());
    b[12..16].copy_from_slice(&(BLOCK as u32).to_le_bytes());
    b[16..32].copy_from_slice(&h.uuid);
    put64(&mut b, 32, h.epoch);
    b[40..56].copy_from_slice(&h.commit.to_le_bytes());
    put64(&mut b, 56, DATA_START);
    put64(&mut b, 64, 3);
    match layout {
        WalLayout::Dense => seal(&mut b, b"ULMDB/hybrid/header/v1\0"),
        WalLayout::PreSized { capacity } => {
            b[..8].copy_from_slice(b"ULMDBHW2");
            b[8..12].copy_from_slice(&2u32.to_le_bytes());
            put64(&mut b, 64, 7);
            put64(&mut b, 72, capacity);
            seal(&mut b, b"ULMDB/hybrid/header/v2\0");
        }
    }
    Ok(b)
}
pub fn decode_header(b: &[u8; BLOCK]) -> Result<Bootstrap> {
    let (h, layout) = decode_layout_header(b)?;
    if layout != WalLayout::Dense {
        return Err(Error::Corrupt("hybrid expected dense header"));
    }
    Ok(h)
}
pub fn decode_layout_header(b: &[u8; BLOCK]) -> Result<(Bootstrap, WalLayout)> {
    let layout = match &b[..8] {
        b"ULMDBHW1" => WalLayout::Dense,
        b"ULMDBHW2" => WalLayout::PreSized {
            capacity: get64(b, 72),
        },
        _ => return Err(Error::Corrupt("hybrid WAL layout tag")),
    };
    let h = Bootstrap {
        uuid: b[16..32].try_into().unwrap(),
        epoch: get64(b, 32),
        commit: u128::from_le_bytes(b[40..56].try_into().unwrap()),
    };
    if *b
        != encode_layout_header(h, layout)
            .map_err(|_| Error::Corrupt("hybrid bootstrap identity"))?
    {
        return Err(Error::Corrupt("hybrid global header/version/hash"));
    }
    Ok((h, layout))
}
pub fn genesis(h: Bootstrap) -> Result<Witness> {
    genesis_layout(h, WalLayout::Dense)
}
pub fn genesis_layout(h: Bootstrap, layout: WalLayout) -> Result<Witness> {
    layout.validate()?;
    // Logical genesis is layout-independent; physical header/witness bind geometry.
    let b = encode_header(h)?;
    Ok(Witness {
        receipt: WalReceipt {
            lsn: 0,
            sequence: 0,
            offset: DATA_START,
            length: 0,
            digest: b[224..].try_into().unwrap(),
        },
        commit: h.commit,
    })
}
pub fn encode_witness(h: Bootstrap, w: Witness) -> [u8; BLOCK] {
    encode_layout_witness(h, w, WalLayout::Dense)
}
pub fn encode_layout_witness(h: Bootstrap, w: Witness, layout: WalLayout) -> [u8; BLOCK] {
    let mut b = [0; BLOCK];
    b[..8].copy_from_slice(b"ULMDBHS1");
    b[8..12].copy_from_slice(&1u32.to_le_bytes());
    b[12..16].copy_from_slice(&(BLOCK as u32).to_le_bytes());
    b[16..32].copy_from_slice(&h.uuid);
    put64(&mut b, 32, h.epoch);
    put64(&mut b, 48, w.receipt.lsn);
    put64(&mut b, 56, w.receipt.sequence);
    put64(&mut b, 64, w.receipt.offset);
    put64(&mut b, 72, w.receipt.length);
    b[80..112].copy_from_slice(&w.receipt.digest);
    b[112..128].copy_from_slice(&w.commit.to_le_bytes());
    match layout {
        WalLayout::Dense => seal(&mut b, b"ULMDB/hybrid/selected/v1\0"),
        WalLayout::PreSized { capacity } => {
            put64(&mut b, 128, capacity);
            b[..8].copy_from_slice(b"ULMDBHS2");
            b[8..12].copy_from_slice(&2u32.to_le_bytes());
            seal(&mut b, b"ULMDB/hybrid/selected/v2\0");
        }
    }
    b
}
pub fn decode_witness(h: Bootstrap, b: &[u8; BLOCK]) -> Result<Witness> {
    decode_layout_witness(h, b, WalLayout::Dense)
}
pub fn decode_layout_witness(h: Bootstrap, b: &[u8; BLOCK], layout: WalLayout) -> Result<Witness> {
    let w = Witness {
        receipt: WalReceipt {
            lsn: get64(b, 48),
            sequence: get64(b, 56),
            offset: get64(b, 64),
            length: get64(b, 72),
            digest: b[80..112].try_into().unwrap(),
        },
        commit: u128::from_le_bytes(b[112..128].try_into().unwrap()),
    };
    if *b != encode_layout_witness(h, w, layout) {
        return Err(Error::Corrupt("hybrid witness version/hash/reserved"));
    }
    if w.receipt.lsn == 0 {
        if w != genesis_layout(h, layout)? {
            return Err(Error::Corrupt("hybrid noncanonical genesis witness"));
        }
    } else if w.commit == 0
        || w.receipt.sequence == 0
        || w.receipt.offset < DATA_START
        || !w.receipt.offset.is_multiple_of(8)
        || w.receipt.length < 320
        || !w.receipt.length.is_multiple_of(8)
        || w.receipt.end()? > layout.limit()
    {
        return Err(Error::Corrupt("hybrid witness extent"));
    }
    Ok(w)
}
pub fn empty_root() -> RootState {
    RootState {
        catalog: Reference::NULL,
        next_chunk: 1,
        next_database: 1,
    }
}
