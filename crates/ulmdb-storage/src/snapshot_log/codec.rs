//! Bounded encoding for short redo records with repeated byte runs.
//!
//! Large transactions bypass this codec. Small records are encoded only when
//! the complete stored body saves at least32 bytes; otherwise the raw frame
//! remains authoritative. The checksum authenticates the stored representation
//! before any decoder length can cause an allocation.

use crate::{Error, Result};

pub(super) const MIN_INPUT: usize = 64;
pub(super) const MAX_INPUT: usize = 2048;
pub(super) const SCRATCH_BYTES: usize = MAX_INPUT + MAX_INPUT / 128 + 4;
const MIN_SAVING: usize = 32;

/// Stored body: u32 decoded length, followed by literal/run blocks. A control
/// byte below128 copies control+1 literals; a control byte >=128 repeats its
/// following byte (control&127)+3 times. There are no backward references.
pub(super) fn encode<'a>(input: &[u8], scratch: &'a mut [u8; SCRATCH_BYTES]) -> Option<&'a [u8]> {
    if !(MIN_INPUT..=MAX_INPUT).contains(&input.len()) {
        return None;
    }
    scratch[..4].copy_from_slice(&(input.len() as u32).to_le_bytes());
    let (mut from, mut to) = (0, 4);
    while from < input.len() {
        let run = repeated(input, from);
        if run >= 3 {
            scratch[to] = 128 | (run - 3) as u8;
            scratch[to + 1] = input[from];
            to += 2;
            from += run;
        } else {
            let start = from;
            from += run;
            while from < input.len() && from - start < 128 {
                let run = repeated(input, from);
                if run >= 3 {
                    break;
                }
                from += run.min(128 - (from - start));
            }
            let length = from - start;
            scratch[to] = (length - 1) as u8;
            to += 1;
            scratch[to..to + length].copy_from_slice(&input[start..from]);
            to += length;
        }
        // An encoded prefix cannot shrink later. Refuse before considering a
        // larger scratch write and avoid scanning an incompressible tail.
        if to > input.len() - MIN_SAVING {
            return None;
        }
    }
    Some(&scratch[..to])
}

fn repeated(input: &[u8], from: usize) -> usize {
    let end = input.len().min(from + 130);
    let mut at = from + 1;
    while at < end && input[at] == input[from] {
        at += 1;
    }
    at - from
}

/// Validate the allocation bound separately so Journal can also enforce its
/// aggregate decoded replay budget before allocating this record's bytes.
pub(super) fn decoded_length(stored: &[u8]) -> Result<usize> {
    let prefix = stored
        .get(..4)
        .ok_or(Error::Corrupt("snapshot log compressed length"))?;
    let length = u32::from_le_bytes(prefix.try_into().unwrap()) as usize;
    if !(MIN_INPUT..=MAX_INPUT).contains(&length)
        || stored
            .len()
            .checked_add(MIN_SAVING)
            .is_none_or(|n| n > length)
    {
        return Err(Error::Corrupt("snapshot log compressed bound"));
    }
    Ok(length)
}

pub(super) fn decode(stored: &[u8]) -> Result<Vec<u8>> {
    let length = decoded_length(stored)?;
    let mut output = Vec::new();
    output
        .try_reserve_exact(length)
        .map_err(|_| Error::LimitExceeded)?;
    let mut at = 4;
    while at < stored.len() {
        let control = stored[at];
        at += 1;
        if control < 128 {
            let count = control as usize + 1;
            let literals = stored
                .get(at..at + count)
                .ok_or(Error::Corrupt("snapshot log compressed literal"))?;
            if output.len() + count > length {
                return Err(Error::Corrupt("snapshot log compressed expansion"));
            }
            output.extend_from_slice(literals);
            at += count;
        } else {
            let count = (control & 127) as usize + 3;
            let byte = *stored
                .get(at)
                .ok_or(Error::Corrupt("snapshot log compressed run"))?;
            at += 1;
            if output.len() + count > length {
                return Err(Error::Corrupt("snapshot log compressed expansion"));
            }
            output.resize(output.len() + count, byte);
        }
    }
    if output.len() != length {
        return Err(Error::Corrupt("snapshot log compressed incomplete"));
    }
    Ok(output)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn all_eligible_lengths_and_binary_boundaries_round_trip() {
        let mut scratch = [0; SCRATCH_BYTES];
        for length in MIN_INPUT..=MAX_INPUT {
            let mut input = vec![0; length];
            for (at, byte) in input.iter_mut().enumerate() {
                *byte = match at % 139 {
                    0..=4 => at as u8,
                    _ => (at / 139) as u8,
                };
            }
            let encoded = encode(&input, &mut scratch).unwrap();
            assert!(encoded.len() + MIN_SAVING <= input.len());
            assert_eq!(decode(encoded).unwrap(), input);
        }
        for byte in 0..=255 {
            let input = vec![byte; MAX_INPUT];
            let encoded = encode(&input, &mut scratch).unwrap();
            assert_eq!(decode(encoded).unwrap(), input);
        }
    }

    #[test]
    fn raw_and_large_inputs_bypass_without_expansion() {
        let mut scratch = [0; SCRATCH_BYTES];
        for length in 0..=MAX_INPUT + 1 {
            let input: Vec<_> = (0..length).map(|at| at as u8).collect();
            assert!(encode(&input, &mut scratch).is_none());
        }
        assert!(encode(&[7; MIN_INPUT - 1], &mut scratch).is_none());
        assert!(encode(&[7; MAX_INPUT + 1], &mut scratch).is_none());
    }

    #[test]
    fn malformed_blocks_and_expansion_bounds_are_rejected() {
        for stored in [
            vec![],
            vec![0; 4],
            u32::MAX.to_le_bytes().to_vec(),
            {
                let mut data = 96u32.to_le_bytes().to_vec();
                data.push(128);
                data
            },
            {
                let mut data = 96u32.to_le_bytes().to_vec();
                data.push(30);
                data
            },
            {
                let mut data = 96u32.to_le_bytes().to_vec();
                data.extend([255, 7]);
                data
            },
            {
                let mut data = 96u32.to_le_bytes().to_vec();
                data.extend([128, 7]);
                data
            },
        ] {
            assert!(decode(&stored).is_err());
        }
    }

    #[test]
    fn literal_blocks_mixed_with_short_and_maximum_runs_are_exact() {
        let mut input: Vec<u8> = (0..128).collect();
        input.extend([128; 2]);
        input.extend([255; 131]);
        input.extend((0..128).rev());
        input.extend([0; 130]);
        let mut scratch = [0; SCRATCH_BYTES];
        let encoded = encode(&input, &mut scratch).unwrap();
        assert_eq!(decode(encoded).unwrap(), input);
    }
}
