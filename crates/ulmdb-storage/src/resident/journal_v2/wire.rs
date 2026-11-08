//! New negotiated resident literal-WAL family; never an old append decoder mode.
use crate::hybrid::redo::WalReceipt;
use crate::resident::wal_codec_v2::{self, Boot};
use crate::{Error, Result};
use sha2::{Digest, Sha256};
pub const BLOCK: usize = 256;
pub const DATA_START: u64 = 196608;
pub const HEADS: [u64; 2] = [65536, 131072];
const HEADER: &[u8; 8] = b"ULMDBRW2";
const HEAD: &[u8; 8] = b"ULMDBRS2";
pub const MAX_WAL_BYTES: u64 = 268_435_456;
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
fn u32at(b: &[u8], at: usize) -> u32 {
    u32::from_le_bytes(b[at..at + 4].try_into().unwrap())
}
fn u64at(b: &[u8], at: usize) -> u64 {
    u64::from_le_bytes(b[at..at + 8].try_into().unwrap())
}
fn u128at(b: &[u8], at: usize) -> u128 {
    u128::from_le_bytes(b[at..at + 16].try_into().unwrap())
}
fn u32put(b: &mut [u8], at: usize, v: u32) {
    b[at..at + 4].copy_from_slice(&v.to_le_bytes());
}
fn u64put(b: &mut [u8], at: usize, v: u64) {
    b[at..at + 8].copy_from_slice(&v.to_le_bytes());
}
fn u128put(b: &mut [u8], at: usize, v: u128) {
    b[at..at + 16].copy_from_slice(&v.to_le_bytes());
}
pub fn header(boot: Boot, capacity: u64) -> Result<[u8; BLOCK]> {
    if boot.uuid == [0; 16]
        || boot.epoch == 0
        || boot.commit == 0
        || boot.kv_pages == 0
        || boot
            .kv_pages
            .checked_add(boot.tiny_pages)
            .is_none_or(|n| n > wal_codec_v2::MAX_SLOTS as u32)
        || !(DATA_START..=MAX_WAL_BYTES).contains(&capacity)
        || !capacity.is_multiple_of(4096)
    {
        return Err(Error::InvalidInput("resident WAL boot/capacity"));
    }
    let mut b = [0; BLOCK];
    b[..8].copy_from_slice(HEADER);
    u32put(&mut b, 8, 2);
    u32put(&mut b, 12, BLOCK as u32);
    b[16..32].copy_from_slice(&boot.uuid);
    u64put(&mut b, 32, boot.epoch);
    u128put(&mut b, 40, boot.commit);
    u32put(&mut b, 56, boot.kv_pages);
    u32put(&mut b, 60, boot.tiny_pages);
    u64put(&mut b, 64, capacity);
    u64put(&mut b, 72, DATA_START);
    u32put(&mut b, 80, 4096);
    u32put(&mut b, 84, 28);
    u64put(&mut b, 88, 31);
    let d = hash(b"ULMDB/resident/WAL-header/v2\0", &b[..224]);
    b[224..].copy_from_slice(&d);
    Ok(b)
}
pub(super) fn seed(bytes: &[u8]) -> Option<crate::resident::checkpoint::Seed> {
    if &bytes[..8] == b"ULMDBRW4" {
        Some(crate::resident::checkpoint::Seed::decode(&bytes[96..184]))
    } else {
        None
    }
}
pub(super) fn header_seeded(
    boot: Boot,
    capacity: u64,
    seed: crate::resident::checkpoint::Seed,
) -> Result<[u8; BLOCK]> {
    seed.validate(boot)?;
    let mut h = header(boot, capacity)?;
    h[..8].copy_from_slice(b"ULMDBRW4");
    seed.encode(&mut h[96..184]);
    let d = hash(b"ULMDB/resident/checkpoint-WAL-header/v1\0", &h[..224]);
    h[224..].copy_from_slice(&d);
    Ok(h)
}
pub fn decode_header(bytes: &[u8]) -> Result<(Boot, u64)> {
    if bytes.len() != BLOCK {
        return Err(Error::Corrupt("resident WAL header width"));
    }
    let boot = Boot {
        uuid: bytes[16..32].try_into().unwrap(),
        epoch: u64at(bytes, 32),
        commit: u128at(bytes, 40),
        kv_pages: u32at(bytes, 56),
        tiny_pages: u32at(bytes, 60),
    };
    let capacity = u64at(bytes, 64);
    let mut expected = match seed(bytes) {
        Some(seed) => header_seeded(boot, capacity, seed),
        None => header(boot, capacity),
    }
    .map_err(|_| Error::Corrupt("resident WAL header configuration"))?;
    if let Some(identity) = crate::resident::capture::protocol(bytes)? {
        expected = crate::resident::capture::negotiated(expected, identity);
    }
    if expected != bytes {
        return Err(Error::Corrupt("resident WAL header type/hash/reserved"));
    }
    Ok((boot, capacity))
}
pub fn head(boot: Boot, capacity: u64, w: Witness) -> [u8; BLOCK] {
    let mut b = [0; BLOCK];
    b[..8].copy_from_slice(HEAD);
    u32put(&mut b, 8, 2);
    u32put(&mut b, 12, BLOCK as u32);
    b[16..32].copy_from_slice(&boot.uuid);
    u64put(&mut b, 32, boot.epoch);
    u64put(&mut b, 40, w.receipt.lsn);
    u64put(&mut b, 48, w.receipt.sequence);
    u64put(&mut b, 56, w.receipt.offset);
    u64put(&mut b, 64, w.receipt.length);
    b[72..104].copy_from_slice(&w.receipt.digest);
    u128put(&mut b, 104, w.commit);
    u64put(&mut b, 120, capacity);
    u32put(&mut b, 128, boot.kv_pages);
    u32put(&mut b, 132, boot.tiny_pages);
    let d = hash(b"ULMDB/resident/WAL-witness/v2\0", &b[..224]);
    b[224..].copy_from_slice(&d);
    b
}
pub fn decode_head(bytes: &[u8], boot: Boot, capacity: u64) -> Result<Witness> {
    if bytes.len() != BLOCK {
        return Err(Error::Corrupt("resident WAL witness width"));
    }
    let receipt = WalReceipt {
        lsn: u64at(bytes, 40),
        sequence: u64at(bytes, 48),
        offset: u64at(bytes, 56),
        length: u64at(bytes, 64),
        digest: bytes[72..104].try_into().unwrap(),
    };
    let w = Witness {
        receipt,
        commit: u128at(bytes, 104),
    };
    if head(boot, capacity, w) != bytes {
        return Err(Error::Corrupt("resident WAL witness type/hash/reserved"));
    }
    if receipt.lsn == 0 {
        if receipt != wal_codec_v2::genesis(&boot)? || w.commit != boot.commit {
            return Err(Error::Corrupt("resident WAL genesis witness"));
        }
    } else if receipt.sequence > receipt.lsn
        || (receipt.length < (wal_codec_v2::HEADER + wal_codec_v2::FOOTER) as u64
            && !(receipt.length == 0 && receipt.offset == DATA_START && receipt.digest != [0; 32]))
        || !receipt.length.is_multiple_of(8)
        || !receipt.offset.is_multiple_of(8)
        || receipt.offset < DATA_START
        || receipt.end()? > capacity
        || w.commit
            != boot
                .commit
                .checked_add(receipt.sequence as u128)
                .ok_or(Error::LimitExceeded)?
    {
        return Err(Error::Corrupt("resident WAL witness extent/identity"));
    }
    Ok(w)
}

#[cfg(test)]
mod capacity_tests {
    use super::*;
    #[test]
    fn full_windows_capture_identity_is_authenticated_in_compact_header() {
        crate::resident::capture::check_full_identity_codec(header, decode_header);
    }
    #[test]
    fn expanded_boot_headers_preserve_old_bytes_and_reject_overflow() {
        let old = Boot {
            uuid: [7; 16],
            epoch: 2,
            commit: 100,
            kv_pages: 4096,
            tiny_pages: 0,
        };
        let bytes = header(old, 1 << 20).unwrap();
        let digest: [u8; 32] = Sha256::digest(bytes).into();
        assert_eq!(
            digest,
            [
                181, 91, 162, 0, 86, 171, 226, 45, 150, 126, 46, 182, 254, 160, 139, 70, 2, 18,
                231, 10, 230, 55, 195, 209, 210, 246, 192, 178, 105, 166, 38, 46
            ]
        );
        for count in [4096, 4097, wal_codec_v2::MAX_SLOTS as u32] {
            let boot = Boot {
                kv_pages: count,
                ..old
            };
            assert_eq!(
                decode_header(&header(boot, 1 << 20).unwrap()).unwrap(),
                (boot, 1 << 20)
            );
            let witness = Witness {
                receipt: wal_codec_v2::genesis(&boot).unwrap(),
                commit: boot.commit,
            };
            assert_eq!(
                decode_head(&head(boot, 1 << 20, witness), boot, 1 << 20).unwrap(),
                witness
            );
        }
        for invalid in [
            Boot {
                kv_pages: wal_codec_v2::MAX_SLOTS as u32 + 1,
                ..old
            },
            Boot {
                kv_pages: wal_codec_v2::MAX_SLOTS as u32,
                tiny_pages: 1,
                ..old
            },
            Boot {
                kv_pages: u32::MAX,
                tiny_pages: 1,
                ..old
            },
        ] {
            assert!(header(invalid, 1 << 20).is_err());
        }
        for (field, value) in [(56, wal_codec_v2::MAX_SLOTS as u32 + 1), (60, u32::MAX)] {
            let mut forged = bytes;
            forged[field..field + 4].copy_from_slice(&value.to_le_bytes());
            let d = hash(b"ULMDB/resident/WAL-header/v2\0", &forged[..224]);
            forged[224..].copy_from_slice(&d);
            assert!(decode_header(&forged).is_err());
        }
    }
}
