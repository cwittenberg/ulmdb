//! Static directory-selection strategies. Decode only exact canonical names;
//! unknown spellings are left untouched, including uppercase aliases.
use super::Kind;
use std::collections::BTreeSet;

pub(super) enum Selection<'a> {
    Segments(&'a BTreeSet<[u8; 32]>),
    Descriptors(&'a BTreeSet<u128>),
    Cohorts(&'a BTreeSet<u128>),
    Temporary,
}
fn canonical_hex<const N: usize>(name: &str, suffix: &str) -> Option<[u8; N]> {
    let digits = name.strip_suffix(suffix)?.as_bytes();
    if digits.len() != N * 2 {
        return None;
    }
    let mut decoded = [0; N];
    for (output, pair) in decoded.iter_mut().zip(digits.chunks_exact(2)) {
        *output = ulmdb_core::hex::lowercase_byte(pair[0], pair[1])?;
    }
    Some(decoded)
}
pub(super) fn temporary(name: &str) -> bool {
    name.starts_with(".prepare-") && name.ends_with(".tmp")
}
impl Selection<'_> {
    pub(super) fn candidate(&self, name: &str) -> Option<Kind> {
        if temporary(name) {
            return Some(Kind::Temporary);
        }
        match self {
            Self::Segments(keep) => {
                let hash = canonical_hex::<32>(name, ".seg")?;
                (!keep.contains(&hash)).then_some(Kind::Segment)
            }
            Self::Descriptors(keep) => {
                let generation = u128::from_be_bytes(canonical_hex::<16>(name, ".root")?);
                (!keep.contains(&generation)).then_some(Kind::Descriptor)
            }
            Self::Cohorts(keep) => {
                let generation = u128::from_be_bytes(canonical_hex::<16>(name, ".cohort")?);
                (!keep.contains(&generation)).then_some(Kind::Cohort)
            }
            // The transactions directory is exclusively writer-private. Its
            // existing rule removes every regular file, even unknown names.
            Self::Temporary => Some(Kind::Temporary),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn complete_hash_and_big_endian_generation_identities_are_distinct() {
        let mut hash = [0; 32];
        hash[31] = 0x0a;
        let hashes = BTreeSet::from([hash]);
        let segments = Selection::Segments(&hashes);
        assert!(
            segments
                .candidate(&format!("{}0a.seg", "0".repeat(62)))
                .is_none()
        );
        // The same low 128 bits must not alias a different full SHA-256 ID.
        assert!(matches!(
            segments.candidate(&format!("{}{}0a.seg", "f".repeat(32), "0".repeat(30))),
            Some(Kind::Segment)
        ));
        let generations = BTreeSet::from([0x0a]);
        let roots = Selection::Descriptors(&generations);
        assert!(roots.candidate(&format!("{:032x}.root", 0x0a)).is_none());
        assert!(matches!(
            roots.candidate(&format!("0a{}.root", "0".repeat(30))),
            Some(Kind::Descriptor)
        ));
        assert!(matches!(
            Selection::Cohorts(&BTreeSet::new()).candidate(&format!("{:032x}.cohort", 0x0a)),
            Some(Kind::Cohort)
        ));
    }
}
