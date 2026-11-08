//! Frozen actual decoder is the native correspondence oracle; not an abstract trie.
#[allow(dead_code)]
#[path = "frozen_node.rs"]
mod frozen;
use crate::node::Node;
use std::cell::Cell;
use ulmdb_core::{
    Address, Prefix,
    storage::{Error, ReadStore, Reference, Result},
};
struct Bytes<'a> {
    raw: &'a [u8],
    reads: Cell<usize>,
    io: bool,
}
impl ReadStore for Bytes<'_> {
    fn read(&self, _at: Reference, len: usize) -> Result<&[u8]> {
        assert_eq!(len, 96);
        self.reads.set(self.reads.get() + 1);
        if self.io {
            return Err(Error::Io(std::io::Error::from_raw_os_error(5)));
        }
        Ok(self.raw)
    }
}
fn check(raw: &[u8], at: Reference, io: bool) {
    let a = Bytes {
        raw,
        reads: Cell::new(0),
        io,
    };
    let b = Bytes {
        raw,
        reads: Cell::new(0),
        io,
    };
    let old = frozen::Node::read(&a, at);
    let new = Node::read(&b, at);
    assert_eq!(a.reads.get(), b.reads.get());
    match (old, new) {
        (Ok(a), Ok(b)) => assert_eq!(
            (a.prefix, a.children, a.value),
            (b.prefix, b.children, b.value)
        ),
        (Err(a), Err(b)) => assert_eq!(a.to_string(), b.to_string()),
        (a, b) => panic!(
            "decoder correspondence differs: old {:?},new {:?}",
            a.map(|x| x.prefix),
            b.map(|x| x.prefix)
        ),
    }
}
fn good(v6: bool, length: u8) -> [u8; 96] {
    let address = if v6 {
        Address::V6(0x20010db81234567890abcdef12345678)
    } else {
        Address::V4(0xace13377)
    };
    let prefix = Prefix::normalize(address, length).unwrap();
    frozen::Node {
        prefix,
        children: [Reference::NULL; 2],
        value: Reference {
            segment: 1,
            offset: 16,
        },
    }
    .encode()
}
#[test]
fn all_byte_values_and_lengths_match_frozen_decoder_and_error_order() {
    let at = Reference {
        segment: 1,
        offset: 16,
    };
    let mut comparisons = 0;
    for (v6, length) in [
        (false, 0),
        (false, 24),
        (false, 32),
        (true, 0),
        (true, 64),
        (true, 128),
    ] {
        let raw = good(v6, length);
        check(&raw, at, false);
        for index in 0..96 {
            for byte in 0..=255u8 {
                let mut changed = raw;
                changed[index] = byte;
                check(&changed, at, false);
                comparisons += 1;
            }
        }
        for len in 0..=112 {
            let mut changed = raw.to_vec();
            changed.resize(len, 0);
            check(&changed, at, false);
            comparisons += 1;
        }
    }
    assert_eq!(comparisons, 148134);
}
#[test]
fn every_family_length_and_reference_tuple_preserves_full_decode() {
    let at = Reference {
        segment: 1,
        offset: 16,
    };
    let pairs = [
        (0, 0),
        (1, 0),
        (1, 16),
        (0, 16),
        (1, 1),
        (u64::MAX, u64::MAX - 15),
    ];
    for v6 in [false, true] {
        let width = if v6 { 128 } else { 32 };
        for length in 0..=width {
            let raw = good(v6, length);
            check(&raw, at, false);
            for &(a, b) in &pairs {
                for &(c, d) in &pairs {
                    for &(e, f) in &pairs {
                        let mut bytes = raw;
                        for (offset, (segment, within)) in
                            [(32, (a, b)), (48, (c, d)), (64, (e, f))]
                        {
                            bytes[offset..offset + 8].copy_from_slice(&segment.to_le_bytes());
                            bytes[offset + 8..offset + 16].copy_from_slice(&within.to_le_bytes());
                        }
                        check(&bytes, at, false);
                    }
                }
            }
        }
    }
}
#[test]
fn ref_decode_precedes_alignment_and_store_errors_are_not_absence() {
    let mut bytes = good(false, 32);
    let at = Reference {
        segment: 1,
        offset: 16,
    };
    // A well-formed but unaligned first reference must not suppress a malformed
    // later reference: the old decoder parses all3 refs before any alignment.
    bytes[32..40].copy_from_slice(&1u64.to_le_bytes());
    bytes[40..48].copy_from_slice(&1u64.to_le_bytes());
    bytes[48..56].fill(0);
    bytes[56..64].copy_from_slice(&16u64.to_le_bytes());
    check(&bytes, at, false);
    assert_eq!(
        Node::read(
            &Bytes {
                raw: &bytes,
                reads: Cell::new(0),
                io: false
            },
            at
        )
        .err()
        .unwrap()
        .to_string(),
        "database integrity error: malformed null reference"
    );
    for at in [
        Reference::NULL,
        Reference {
            segment: 0,
            offset: 16,
        },
        Reference {
            segment: 1,
            offset: 1,
        },
        Reference {
            segment: 1,
            offset: 16,
        },
    ] {
        check(&good(true, 64), at, false);
        check(&good(true, 64), at, true);
    }
}
