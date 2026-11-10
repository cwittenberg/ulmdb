//! Local validity of exact 4 KiB V2 bytewise leaves, without graph authority.

const PAGE_SIZE: usize = 4096;
const HEADER: usize = 32;

#[derive(Clone, Copy)]
struct Layout {
    count: usize,
    upper: usize,
    key: usize,
    value: usize,
    stride: usize,
}

fn word(bytes: &[u8], offset: usize) -> Option<u16> {
    let end = offset.checked_add(2)?;
    Some(u16::from_le_bytes(bytes.get(offset..end)?.try_into().ok()?))
}

fn layout(bytes: &[u8]) -> Option<Layout> {
    if bytes.len() != PAGE_SIZE
        || word(bytes, 0)? != 2
        || word(bytes, 2)? != 2
        || word(bytes, 4)? != 0
        || bytes.get(16..HEADER)?.iter().any(|&byte| byte != 0)
    {
        return None;
    }
    let count = word(bytes, 6)?;
    let lower = word(bytes, 8)?;
    let upper = word(bytes, 10)?;
    let key = word(bytes, 12)?;
    let value = word(bytes, 14)?;
    if !crate::compact_page_geometry::valid_geometry(true, count, lower, upper, key, value) {
        return None;
    }
    let key = usize::from(key);
    let value = usize::from(value);
    Some(Layout {
        count: usize::from(count),
        upper: usize::from(upper),
        key,
        value,
        stride: key.checked_add(value)?,
    })
}

impl Layout {
    fn key_at(self, bytes: &[u8], index: usize) -> Option<&[u8]> {
        if index >= self.count {
            return None;
        }
        let start = HEADER.checked_add(index.checked_mul(self.stride)?)?;
        let end = start.checked_add(self.key)?;
        if end > self.upper {
            return None;
        }
        bytes.get(start..end)
    }

    fn capacity(self) -> usize {
        if self.stride == 0 {
            1
        } else {
            (PAGE_SIZE - HEADER) / self.stride
        }
    }
}

/// Check exact V2 leaf geometry, strict bytewise key order, and the zero tail.
/// Unsupported or malformed images return false; KV retains error authority.
/// This issues only local byte validity, never graph or routing authority.
pub fn is_valid(bytes: &[u8]) -> bool {
    let Some(layout) = layout(bytes) else {
        return false;
    };
    let mut previous: Option<&[u8]> = None;
    for index in 0..layout.count {
        let Some(key) = layout.key_at(bytes, index) else {
            return false;
        };
        if previous.is_some_and(|old| old >= key) {
            return false;
        }
        previous = Some(key);
    }
    bytes
        .get(layout.upper..)
        .is_some_and(|tail| tail.iter().all(|&byte| byte == 0))
}

/// Recognize the exact four-span append to an ALREADY CERTIFIED source image.
/// This does not recheck old key order or tail bytes and cannot issue validity.
/// The caller must bind that certificate to these exact current bytes and
/// publish preservation only after all writes succeed. Empty key/value spans
/// are supported; they do not constitute overlapping written bytes.
pub fn preserves_append(bytes: &[u8], patches: &[(usize, &[u8])]) -> bool {
    let Some(layout) = layout(bytes) else {
        return false;
    };
    if patches.len() != 4 || layout.count >= layout.capacity() {
        return false;
    }
    let Some(count) = layout.count.checked_add(1) else {
        return false;
    };
    let Ok(count) = u16::try_from(count) else {
        return false;
    };
    let Some(upper) = layout.upper.checked_add(layout.stride) else {
        return false;
    };
    if upper > PAGE_SIZE {
        return false;
    }
    let Ok(upper_word) = u16::try_from(upper) else {
        return false;
    };
    let Some(value_start) = layout.upper.checked_add(layout.key) else {
        return false;
    };
    let (count_at, count_bytes) = patches[0];
    let (upper_at, upper_bytes) = patches[1];
    let (key_at, key) = patches[2];
    let (value_at, value) = patches[3];
    if count_at != 6
        || count_bytes.len() != 2
        || word(count_bytes, 0) != Some(count)
        || upper_at != 10
        || upper_bytes.len() != 2
        || word(upper_bytes, 0) != Some(upper_word)
        || key_at != layout.upper
        || key.len() != layout.key
        || value_at != value_start
        || value.len() != layout.value
        || value_start.checked_add(value.len()) != Some(upper)
    {
        return false;
    }
    if let Some(last) = layout.count.checked_sub(1) {
        let Some(previous) = layout.key_at(bytes, last) else {
            return false;
        };
        if previous >= key {
            return false;
        }
    }
    true
}

/// Recognize one whole live inline value in an ALREADY CERTIFIED source image.
/// Old key order and tail bytes are assumed, not revalidated. This cannot issue
/// a certificate. Only exact live-value positions are recognized, including
/// empty live values; callers keep their existing generic no-op handling.
pub fn preserves_value_patch(bytes: &[u8], offset: usize, value: &[u8]) -> bool {
    let Some(layout) = layout(bytes) else {
        return false;
    };
    if layout.count == 0 || value.len() != layout.value {
        return false;
    }
    let Some(first) = HEADER.checked_add(layout.key) else {
        return false;
    };
    let Some(relative) = offset.checked_sub(first) else {
        return false;
    };
    if layout.stride == 0 {
        return relative == 0 && layout.count == 1;
    }
    if relative % layout.stride != 0 || relative / layout.stride >= layout.count {
        return false;
    }
    offset
        .checked_add(value.len())
        .is_some_and(|end| end <= layout.upper && end <= bytes.len())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn leaf(key: u16, value: u16, rows: &[(&[u8], &[u8])]) -> [u8; PAGE_SIZE] {
        let mut bytes = [0; PAGE_SIZE];
        let stride = usize::from(key) + usize::from(value);
        let upper = HEADER + rows.len() * stride;
        for (at, number) in [
            (0, 2),
            (2, 2),
            (4, 0),
            (6, rows.len() as u16),
            (8, HEADER as u16),
            (10, upper as u16),
            (12, key),
            (14, value),
        ] {
            bytes[at..at + 2].copy_from_slice(&number.to_le_bytes());
        }
        for (index, &(k, v)) in rows.iter().enumerate() {
            assert_eq!(k.len(), usize::from(key));
            assert_eq!(v.len(), usize::from(value));
            let start = HEADER + index * stride;
            bytes[start..start + k.len()].copy_from_slice(k);
            bytes[start + k.len()..start + stride].copy_from_slice(v);
        }
        bytes
    }

    fn apply(mut bytes: [u8; PAGE_SIZE], patches: &[(usize, &[u8])]) -> [u8; PAGE_SIZE] {
        for &(at, value) in patches {
            bytes[at..at + value.len()].copy_from_slice(value);
        }
        bytes
    }

    #[test]
    fn classifier_rejects_header_order_and_tail_faults() {
        let valid = leaf(3, 2, &[(b"abc", b"12"), (b"abd", b"34")]);
        assert!(is_valid(&valid));
        for at in [0, 2, 4, 6, 8, 10, 12, 14, 16] {
            let mut bad = valid;
            bad[at] ^= 0x80;
            assert!(!is_valid(&bad), "header byte {at}");
        }
        for at in [42, PAGE_SIZE - 1] {
            let mut bad = valid;
            bad[at] = 1;
            assert!(!is_valid(&bad));
        }
        let mut duplicate = valid;
        duplicate[37..40].copy_from_slice(b"abc");
        assert!(!is_valid(&duplicate));
        for length in [0, 1, 31, 32, PAGE_SIZE - 1] {
            assert!(!is_valid(&valid[..length]));
        }
        let mut long = valid.to_vec();
        long.push(0);
        assert!(!is_valid(&long));
    }

    #[test]
    fn append_shape_preserves_odd_rows_and_declines_near_misses() {
        let bytes = leaf(3, 2, &[(b"abc", b"12")]);
        let count = 2u16.to_le_bytes();
        let upper = 42u16.to_le_bytes();
        let patches: [(usize, &[u8]); 4] = [(6, &count), (10, &upper), (37, b"abd"), (40, b"34")];
        assert!(preserves_append(&bytes, &patches));
        assert!(is_valid(&apply(bytes, &patches)));
        let mut wrong = patches;
        wrong[2] = (37, b"abc");
        assert!(!preserves_append(&bytes, &wrong));
        wrong[2] = (37, b"aba");
        assert!(!preserves_append(&bytes, &wrong));
        wrong = patches;
        wrong.swap(0, 1);
        assert!(!preserves_append(&bytes, &wrong));
        wrong = patches;
        wrong[2].0 += 1;
        assert!(!preserves_append(&bytes, &wrong));
        wrong = patches;
        wrong[3] = (40, b"3");
        assert!(!preserves_append(&bytes, &wrong));
        assert!(!preserves_append(&bytes, &patches[..3]));
        let mut extra = patches.to_vec();
        extra.push((0, &[]));
        assert!(!preserves_append(&bytes, &extra));
    }

    #[test]
    fn append_can_fill_the_exact_page_boundary() {
        let keys: [[u8; 2]; 16] = std::array::from_fn(|index| (index as u16).to_be_bytes());
        let value = [7; 252];
        let rows: Vec<_> = keys[..15]
            .iter()
            .map(|key| (key.as_slice(), value.as_slice()))
            .collect();
        let bytes = leaf(2, 252, &rows);
        assert!(is_valid(&bytes));
        let count = 16u16.to_le_bytes();
        let upper = (PAGE_SIZE as u16).to_le_bytes();
        let patches: [(usize, &[u8]); 4] =
            [(6, &count), (10, &upper), (3842, &keys[15]), (3844, &value)];
        assert!(preserves_append(&bytes, &patches));
        let full = apply(bytes, &patches);
        assert!(is_valid(&full));
        assert!(!preserves_append(&full, &patches));
    }

    #[test]
    fn value_patch_accepts_only_complete_live_values() {
        let bytes = leaf(3, 2, &[(b"abc", b"12"), (b"abd", b"34")]);
        for offset in [35, 40] {
            assert!(preserves_value_patch(&bytes, offset, b"zz"));
            assert!(is_valid(&apply(bytes, &[(offset, b"zz")])));
        }
        for (offset, value) in [
            (34, b"zz".as_slice()),
            (35, b"z"),
            (35, b"zzz"),
            (3, b"zz"),
            (42, b"zz"),
            (usize::MAX, b"zz"),
        ] {
            assert!(!preserves_value_patch(&bytes, offset, value));
        }
    }

    #[test]
    fn zero_width_and_zero_stride_are_checked_without_division() {
        let empty = leaf(0, 0, &[]);
        assert!(is_valid(&empty));
        let count = 1u16.to_le_bytes();
        let upper = (HEADER as u16).to_le_bytes();
        let patches: [(usize, &[u8]); 4] = [(6, &count), (10, &upper), (32, &[]), (32, &[])];
        assert!(preserves_append(&empty, &patches));
        let one = apply(empty, &patches);
        assert!(is_valid(&one));
        assert!(!preserves_append(&one, &patches));
        assert!(preserves_value_patch(&one, 32, &[]));
        assert!(!preserves_value_patch(&one, 33, &[]));
        assert!(!preserves_value_patch(&empty, 32, &[]));
        assert!(!is_valid(&leaf(0, 1, &[(&[], &[1]), (&[], &[2])])));
        let zero_value = leaf(3, 0, &[(b"abc", &[]), (b"abd", &[])]);
        assert!(is_valid(&zero_value));
        assert!(preserves_value_patch(&zero_value, 35, &[]));
        assert!(preserves_value_patch(&zero_value, 38, &[]));
        assert!(!preserves_value_patch(&zero_value, 34, &[]));
    }
}
