//! Compare the shared certificate classifier with the existing KV validator.
use super::*;
use std::cell::Cell;
use ulmdb_core::storage::ReadStore;

fn image(key: usize, value: usize, count: usize) -> Vec<u8> {
    let mut bytes = vec![0; 4096];
    for (at, word) in [
        (0, 2),
        (2, 2),
        (6, count as u16),
        (8, 32),
        (10, (32 + count * (key + value)) as u16),
        (12, key as u16),
        (14, value as u16),
    ] {
        bytes[at..at + 2].copy_from_slice(&word.to_le_bytes());
    }
    for row in 0..count {
        let start = 32 + row * (key + value);
        assert!(start + key + value <= bytes.len());
        let number = (row as u64).to_be_bytes();
        let digits = key.min(number.len());
        bytes[start + key - digits..start + key].copy_from_slice(&number[number.len() - digits..]);
        for (index, byte) in bytes[start + key..start + key + value]
            .iter_mut()
            .enumerate()
        {
            *byte = (row.wrapping_mul(37) ^ index) as u8;
        }
    }
    bytes
}

fn existing_accepts(bytes: &[u8]) -> bool {
    Page::decode(bytes).is_ok_and(|page| {
        page.level == 0 && page.compact().is_some() && page.validate_compact().is_ok()
    })
}

fn compare(bytes: &[u8]) {
    assert_eq!(
        ulmdb_core::compact_leaf::is_valid(bytes),
        existing_accepts(bytes),
        "shared classifier differs from the existing KV validator"
    );
}

#[test]
fn classifier_matches_existing_validator_across_widths_and_header_faults() {
    for key in [0usize, 1, 2, 8, 9, 256] {
        for value in [0usize, 1, 8, 17, 512] {
            let stride = key + value;
            let capacity = if stride == 0 { 1 } else { 4064 / stride };
            let ordered_capacity = if key == 0 {
                capacity.min(1)
            } else if key == 1 {
                capacity.min(256)
            } else {
                capacity
            };
            for count in [0, 1, ordered_capacity.min(7), ordered_capacity] {
                let mut bytes = image(key, value, count);
                assert!(existing_accepts(&bytes));
                compare(&bytes);
                for offset in 0..32 {
                    for bit in 0..8 {
                        bytes[offset] ^= 1 << bit;
                        compare(&bytes);
                        bytes[offset] ^= 1 << bit;
                    }
                }
                let upper = 32 + count * stride;
                for offset in [upper, upper + (4096 - upper) / 2, 4095] {
                    if offset >= upper && offset < bytes.len() {
                        bytes[offset] ^= 0x80;
                        assert!(!existing_accepts(&bytes));
                        compare(&bytes);
                        bytes[offset] ^= 0x80;
                    }
                }
            }
        }
    }
}

#[test]
fn classifier_preserves_existing_order_and_complete_image_checks() {
    for (key, value, count) in [(1, 0, 256), (2, 1, 200), (8, 8, 254), (9, 17, 100)] {
        let mut bytes = image(key, value, count);
        for offset in 32..bytes.len() {
            for mask in [1, 0x80] {
                bytes[offset] ^= mask;
                compare(&bytes);
                bytes[offset] ^= mask;
            }
        }
        let stride = key + value;
        let first_key = bytes[32..32 + key].to_vec();
        bytes[32 + stride..32 + stride + key].copy_from_slice(&first_key);
        assert!(!existing_accepts(&bytes));
        compare(&bytes);
    }
    for length in [0, 31, 32, 4095, 4097, 65536] {
        let mut bytes = image(8, 8, 4);
        bytes.resize(length, 0);
        compare(&bytes);
    }
    let legacy = page::encode_v1(0, Reference::NULL, &[]).unwrap();
    assert!(!ulmdb_core::compact_leaf::is_valid(&legacy));
    compare(&legacy);
}

struct DefaultStore(Cell<usize>);
impl ReadStore for DefaultStore {
    fn read(&self, _: Reference, _: usize) -> Result<&[u8]> {
        self.0.set(self.0.get() + 1);
        Err(Error::Corrupt("unexpected admission read"))
    }
}
impl WriteStore for DefaultStore {
    fn allocate(&mut self, _: &[u8], _: usize) -> Result<Reference> {
        Err(Error::LimitExceeded)
    }
    fn rewrite(&mut self, _: Reference, _: &[u8], _: usize) -> Result<Reference> {
        Err(Error::LimitExceeded)
    }
}

#[test]
fn default_store_never_grants_authority_or_performs_an_admission_read() {
    let store = DefaultStore(Cell::new(0));
    let bytes = image(8, 8, 4);
    assert!(existing_accepts(&bytes));
    assert!(!store.admitted_compact_leaf(Reference::NULL, &bytes));
    assert_eq!(store.0.get(), 0);
}
