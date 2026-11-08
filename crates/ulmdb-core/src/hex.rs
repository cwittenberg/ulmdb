//! Shared executable lowercase-hex primitives; Verus checks these exact bodies.
#[cfg(not(verus_keep_ghost))]
use verus_builtin_macros::verus;
#[cfg(verus_keep_ghost)]
use vstd::prelude::*;

verus! {

pub open spec fn canonical_digit(byte: u8) -> bool {
    (48 <= byte <= 57) || (97 <= byte <= 102)
}

pub open spec fn digit_value(byte: u8) -> int {
    if 48 <= byte <= 57 { byte as int - 48 }
    else { byte as int - 87 }
}

#[inline]
#[allow(clippy::manual_range_contains)] // Explicit bounds are shared with Verus.
pub fn lowercase_digit(byte: u8) -> (digit: Option<u8>)
    ensures match digit {
        Some(n) => canonical_digit(byte) && n < 16 && n as int == digit_value(byte),
        None => !canonical_digit(byte),
    },
{
    if byte >= 48 && byte <= 57 { Some(byte - 48) }
    else if byte >= 97 && byte <= 102 { Some(byte - 87) }
    else { None }
}

#[inline]
#[allow(clippy::question_mark)] // Explicit early returns carry checked contracts.
pub fn lowercase_byte(high: u8, low: u8) -> (decoded: Option<u8>)
    ensures match decoded {
        Some(value) => canonical_digit(high) && canonical_digit(low)
            && value as int == digit_value(high) * 16 + digit_value(low),
        None => !canonical_digit(high) || !canonical_digit(low),
    },
{
    let h = match lowercase_digit(high) { Some(n) => n, None => return None };
    let l = match lowercase_digit(low) { Some(n) => n, None => return None };
    Some(h * 16 + l)
}
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn every_digit_and_byte_pair_matches_the_standard_parser_and_canonical_format() {
        for byte in 0..=u8::MAX {
            let expected = b"0123456789abcdef"
                .iter()
                .position(|b| *b == byte)
                .map(|n| n as u8);
            assert_eq!(lowercase_digit(byte), expected);
        }
        let mut accepted = 0;
        for high in 0..=u8::MAX {
            for low in 0..=u8::MAX {
                let pair = [high, low];
                let expected = std::str::from_utf8(&pair).ok().and_then(|text| {
                    u8::from_str_radix(text, 16)
                        .ok()
                        .filter(|n| format!("{n:02x}") == text)
                });
                assert_eq!(lowercase_byte(high, low), expected, "{pair:?}");
                accepted += usize::from(expected.is_some());
            }
        }
        assert_eq!(accepted, 256);
    }
}
