//! Canonical unsigned LEB128 fields for semantic transaction format2.
//! No decoder length causes allocation until its caller applies field limits.
use super::Input;
use crate::{Error, Result};

pub(super) fn put(out: &mut Vec<u8>, mut value: u64, limit: usize) -> Result<()> {
    let mut bytes = [0; 10];
    let mut count = 0;
    loop {
        let byte = (value & 127) as u8;
        value >>= 7;
        bytes[count] = byte | if value == 0 { 0 } else { 128 };
        count += 1;
        if value == 0 {
            break;
        }
    }
    super::push(out, &bytes[..count], limit)
}

pub(super) fn get(input: &mut Input<'_>) -> Result<u64> {
    let mut value = 0u64;
    for position in 0..10 {
        let byte = input.take(1)?[0];
        if position == 9 && byte > 1 {
            return Err(Error::Corrupt("snapshot-log varint overflow"));
        }
        value |= u64::from(byte & 127) << (position * 7);
        if byte < 128 {
            if position != 0 && byte == 0 {
                return Err(Error::Corrupt("snapshot-log noncanonical varint"));
            }
            return Ok(value);
        }
    }
    Err(Error::Corrupt("snapshot-log varint overflow"))
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn canonical_boundaries_round_trip_without_extra_bytes() {
        for value in [
            0,
            1,
            127,
            128,
            129,
            16383,
            16384,
            u32::MAX as u64,
            1u64 << 63,
            u64::MAX,
        ] {
            let mut out = Vec::new();
            put(&mut out, value, 10).unwrap();
            let expected = if value == 0 {
                1
            } else {
                value.ilog2() as usize / 7 + 1
            };
            assert_eq!(out.len(), expected);
            let mut input = Input { bytes: &out, at: 0 };
            assert_eq!(get(&mut input).unwrap(), value);
            input.finish().unwrap();
        }
    }
    #[test]
    fn overlong_truncated_and_overflow_fields_are_rejected() {
        for bytes in [
            vec![],
            vec![128],
            vec![128, 0],
            vec![129, 0],
            vec![128; 10],
            {
                let mut b = vec![255; 9];
                b.push(2);
                b
            },
            {
                let mut b = vec![128; 9];
                b.push(0);
                b
            },
            {
                let mut b = vec![255; 10];
                b.push(0);
                b
            },
        ] {
            assert!(get(&mut Input {
                bytes: &bytes,
                at: 0
            })
            .is_err());
        }
        let mut out = Vec::new();
        assert!(put(&mut out, u64::MAX, 9).is_err());
        assert!(out.is_empty());
    }
}
